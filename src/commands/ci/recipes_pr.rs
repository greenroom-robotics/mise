use clap::Args;
use std::path::PathBuf;

/// Called by semantic-release's @semantic-release/exec plugin once a new version
/// has been determined. Opens/updates a PR on the recipes repo with the new pin.
#[derive(Args, Debug)]
pub struct RecipesPr {
    /// Release version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: String,
    /// owner/repo of the recipes repository.
    #[arg(long, default_value = "greenroom-robotics/ros-recipes")]
    pub recipes_repo: String,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// Single package, used when semantic-release ran in multi-package mode.
    #[arg(long)]
    pub package: Option<String>,
    /// ROS distro identifier.
    #[arg(long, default_value = "kilted")]
    pub ros_distro: String,
}

impl RecipesPr {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::commands::ci::{packages, pixi_meta, recipes_upsert};

        // 1. Resolve which packages we're upserting and read each one's name.
        //    semantic-release always invokes us with a known version; in
        //    single-package mode --package is set; in multi-package mode the
        //    plugin's cwd is the package being released, so --package may also
        //    be set there. If --package is empty we upsert every package in
        //    --package-dir at the same version (matches platform_cli's behavior).
        let pixis = packages::discover(&self.package_dir, self.package.as_deref())?;
        if pixis.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }

        // 2. Identify the source repo from the current git remote.
        let src_url = source_repo_https_url()?;
        let src_short = source_repo_short_name(&src_url)?;
        let tag = format!("v{}", self.version);

        // 3. Clone the recipes repo into a tempdir.
        let tmp = tempfile::TempDir::new()?;
        let recipes_root = tmp.path().join("recipes");
        clone_recipes_repo(&self.recipes_repo, &recipes_root)?;

        // 4. Create the release branch.
        let branch = format!("release/{}-v{}", src_short, self.version);
        run_in(&recipes_root, &["git", "checkout", "-b", &branch])?;

        // 5. Upsert each package's entry.
        let recipes_yaml = recipes_root.join("rosdistro_additional_recipes.yaml");
        for pixi in &pixis {
            let pkg = pixi_meta::read(pixi)?;
            recipes_upsert::upsert(
                &recipes_yaml,
                &recipes_upsert::Entry {
                    package: &pkg.name,
                    url: &src_url,
                    tag: &tag,
                    version: &self.version,
                },
            )?;
        }

        // 6. Commit + push + open PR.
        run_in(
            &recipes_root,
            &["git", "add", "rosdistro_additional_recipes.yaml"],
        )?;
        run_in(
            &recipes_root,
            &[
                "git",
                "-c",
                "user.name=greenroom-bot",
                "-c",
                "user.email=greenroom-bot@users.noreply.github.com",
                "commit",
                "-m",
                &format!("release: {} {}", src_short, tag),
            ],
        )?;
        run_in(
            &recipes_root,
            &["git", "push", "--force-with-lease", "origin", &branch],
        )?;

        if pr_exists(&self.recipes_repo, &branch)? {
            println!("PR already exists for {branch}; branch updated.");
            return Ok(());
        }

        run_in(
            &recipes_root,
            &[
                "gh",
                "pr",
                "create",
                "--repo",
                &self.recipes_repo,
                "--base",
                "main",
                "--head",
                &branch,
                "--title",
                &format!("release: {} {}", src_short, tag),
                "--body",
                "Automated by `mise ci recipes-pr`.",
                "--label",
                "automerge",
            ],
        )?;

        Ok(())
    }
}

fn source_repo_https_url() -> anyhow::Result<String> {
    let raw = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()?;
    if !raw.status.success() {
        anyhow::bail!("git config --get remote.origin.url failed");
    }
    let raw = String::from_utf8(raw.stdout)?.trim().to_string();
    if let Some(rest) = raw.strip_prefix("git@github.com:") {
        let rest = rest.trim_end_matches(".git");
        return Ok(format!("https://github.com/{rest}.git"));
    }
    if raw.ends_with(".git") {
        Ok(raw)
    } else {
        Ok(format!("{raw}.git"))
    }
}

fn source_repo_short_name(https_url: &str) -> anyhow::Result<String> {
    let last = https_url
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("could not parse repo name from {https_url}"))?;
    Ok(last.trim_end_matches(".git").to_string())
}

fn clone_recipes_repo(repo: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let url = recipes_repo_https_url(repo);
    let status = std::process::Command::new("git")
        .args(["clone", "--depth=1", "--branch=main", &url])
        .arg(dest)
        .status()?;
    if !status.success() {
        anyhow::bail!("git clone failed for {repo}");
    }
    Ok(())
}

fn recipes_repo_https_url(repo: &str) -> String {
    if let Ok(token) = std::env::var("API_TOKEN_GITHUB").or_else(|_| std::env::var("GITHUB_TOKEN"))
    {
        format!("https://x-access-token:{token}@github.com/{repo}.git")
    } else {
        format!("git@github.com:{repo}.git")
    }
}

fn run_in(cwd: &std::path::Path, argv: &[&str]) -> anyhow::Result<()> {
    let (cmd, rest) = argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty argv"))?;
    let status = std::process::Command::new(cmd)
        .args(rest)
        .current_dir(cwd)
        .status()?;
    if !status.success() {
        anyhow::bail!("{} {:?} failed in {}", cmd, rest, cwd.display());
    }
    Ok(())
}

fn pr_exists(repo: &str, branch: &str) -> anyhow::Result<bool> {
    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--repo",
            repo,
            "--head",
            branch,
            "--json",
            "number",
            "--jq",
            ".[0].number",
        ])
        .output()?;
    if !out.status.success() {
        // gh exits non-zero if no PR — accept that as "doesn't exist"
        return Ok(false);
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}
