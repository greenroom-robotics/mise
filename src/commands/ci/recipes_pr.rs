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
    /// Tagged commit SHA (matches ${nextRelease.gitHead}). Used as source.rev for vendored recipes.
    #[arg(long)]
    pub sha: String,
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

        // 4. Create the release branch. The branch name is intentionally
        //    version-independent so every release of this source repo lands on
        //    the SAME rolling PR (force-pushed each time) rather than opening a
        //    fresh PR per version. A per-version branch leaves superseded PRs
        //    open, and an older one merging after a newer one downgrades the
        //    pin. Mirrors `mise bump`'s `bump/<pkg>` branch.
        //
        //    Multi-package repos get a per-package rolling branch
        //    (release/<repo>/<package>) so each package's release lands on its
        //    own PR. Single-package repos (where --package matches the repo
        //    name, or is absent) stay at release/<repo>.
        let branch = release_branch(&src_short, self.package.as_deref());
        run_in(&recipes_root, &["git", "checkout", "-b", &branch])?;

        // 5. Apply each package's release (vendored recipe or rosdistro upsert).
        use std::collections::BTreeSet;
        let mut changed: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        for pixi in &pixis {
            let pkg = pixi_meta::read(pixi)?;
            // Path from the source-repo root to the dir holding this package's
            // pixi.toml. "" or "." means the package sits at the repo root.
            // recipes-pr runs at the source repo root (cwd); when --package-dir
            // was passed as an absolute path the discovered pixi path is also
            // absolute, so strip the cwd to keep the subdir repo-root-relative.
            let parent = pixi
                .parent()
                .map(|p| {
                    let rel = std::env::current_dir()
                        .ok()
                        .and_then(|cwd| p.strip_prefix(&cwd).ok().map(|r| r.to_owned()))
                        .unwrap_or_else(|| p.to_owned());
                    rel.to_string_lossy().into_owned()
                })
                .unwrap_or_default();
            let subdir = match parent.as_str() {
                "" | "." => None,
                s => Some(s),
            };
            let rel = recipes_upsert::apply_release(
                &recipes_root,
                &pkg.name,
                &src_url,
                &tag,
                &self.version,
                &self.sha,
                subdir,
            )?;
            changed.insert(rel);
        }

        // 6. Commit + push + open PR.
        let mut add_args: Vec<String> = vec!["git".into(), "add".into()];
        add_args.extend(changed.iter().map(|p| p.to_string_lossy().into_owned()));
        run_in(
            &recipes_root,
            &add_args.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
        // Commit as the App bot. The release action exports its identity via
        // GIT_AUTHOR_*/GIT_COMMITTER_*, which git honours natively; we mirror
        // it onto -c so a standalone run still has a usable identity, falling
        // back to the greenroom-bot label only when the env is unset.
        let (git_name, git_email) = git_identity();
        let name_cfg = format!("user.name={git_name}");
        let email_cfg = format!("user.email={git_email}");
        let commit_msg = release_title(&src_short, self.package.as_deref(), &tag);
        run_in(
            &recipes_root,
            &[
                "git",
                "-c",
                &name_cfg,
                "-c",
                &email_cfg,
                "commit",
                "-m",
                &commit_msg,
            ],
        )?;
        // Plain --force, not --force-with-lease: the recipes repo is cloned
        // shallow on `main` only, so there's no remote-tracking ref for the
        // rolling branch and --force-with-lease would reject the push.
        run_in(
            &recipes_root,
            &["git", "push", "--force", "origin", &branch],
        )?;

        let title = release_title(&src_short, self.package.as_deref(), &tag);
        if pr_exists(&self.recipes_repo, &branch)? {
            // The rolling PR already exists from a previous release; refresh its
            // title to the new version so it doesn't show a stale version.
            let edit_args = pr_edit_args(&self.recipes_repo, &branch, &title);
            run_in(
                &recipes_root,
                &edit_args.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            println!("PR already exists for {branch}; branch and title updated.");
        } else {
            let create_args = pr_create_args(&self.recipes_repo, &branch, &title);
            run_in(
                &recipes_root,
                &create_args.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
        }

        // Enable GitHub native auto-merge so the PR lands once CI passes
        // (mirrors `mise bump`'s behavior).
        let automerge_args = pr_automerge_args(&self.recipes_repo, &branch);
        run_in(
            &recipes_root,
            &automerge_args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )?;

        Ok(())
    }
}

/// Version-independent branch for a source repo's rolling release PR.
///
/// Multi-package repos get a per-package branch (`release/<repo>/<package>`) so
/// each package lands on its own rolling PR. Single-package repos — where no
/// `--package` is passed, or it matches the repo name — stay at
/// `release/<repo>` and avoid a redundant `release/mise/mise`.
fn release_branch(src_short: &str, package: Option<&str>) -> String {
    match package {
        Some(pkg) if pkg != src_short => format!("release/{src_short}/{pkg}"),
        _ => format!("release/{src_short}"),
    }
}

/// PR title / commit message, mirroring `release_branch`: per-package repos read
/// `release: <repo>/<package> v<ver>`; single-package repos `release: <repo> v<ver>`.
fn release_title(src_short: &str, package: Option<&str>, tag: &str) -> String {
    match package {
        Some(pkg) if pkg != src_short => format!("release: {src_short}/{pkg} {tag}"),
        _ => format!("release: {src_short} {tag}"),
    }
}

fn pr_edit_args(repo: &str, branch: &str, title: &str) -> Vec<String> {
    ["gh", "pr", "edit", "--repo", repo, branch, "--title", title]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Git author/committer identity for the recipes commit. Prefers the App bot
/// identity the release action exports via GIT_AUTHOR_NAME/EMAIL, falling back
/// to the greenroom-bot label for standalone runs where those aren't set.
fn git_identity() -> (String, String) {
    fn env_or(var: &str, fallback: &str) -> String {
        std::env::var(var)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.to_string())
    }
    (
        env_or("GIT_AUTHOR_NAME", "greenroom-bot"),
        env_or("GIT_AUTHOR_EMAIL", "greenroom-bot@users.noreply.github.com"),
    )
}

/// Gremlin flavor text for PR bodies. The footer stays factual so anyone
/// reading the PR still knows what created it.
const GREMLINS: &[&str] = &[
    "🐉 A gremlin smelled a fresh release and dragged this recipe in by its tail.",
    "🐉 The recipe gremlins have been fed. They demand you click merge.",
    "🐉 *gremlin noises* — new version spotted, recipe updated, snacks expected.",
    "🐉 A wild recipe gremlin appeared and bumped the version while you weren't looking.",
    "🐉 Do not feed the gremlins after midnight. They already shipped this PR anyway.",
    "🐉 The gremlin in the build closet insists this recipe is ready. Trust the gremlin.",
];

fn gremlin_body() -> String {
    // ponytail: nanos-modulo pick, no rng dep needed for flavor text
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0)
        % GREMLINS.len();
    format!("{}\n\nAutomated by `mise ci recipes-pr`.", GREMLINS[idx])
}

fn pr_create_args(repo: &str, branch: &str, title: &str) -> Vec<String> {
    [
        "gh",
        "pr",
        "create",
        "--repo",
        repo,
        "--base",
        "main",
        "--head",
        branch,
        "--title",
        title,
        "--body",
        &gremlin_body(),
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn pr_automerge_args(repo: &str, branch: &str) -> Vec<String> {
    [
        "gh", "pr", "merge", "--repo", repo, branch, "--auto", "--squash",
    ]
    .into_iter()
    .map(String::from)
    .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    // The recipes repo merges bump PRs via GitHub's native auto-merge, not a
    // label — `--label automerge` only attaches a literal label and the PR
    // never merges.
    #[test]
    fn create_args_do_not_use_automerge_label() {
        let args = pr_create_args("greenroom-robotics/ros-recipes", "release/mise", "t");
        assert!(!args.iter().any(|a| a == "--label"));
    }

    #[test]
    fn automerge_args_enable_native_auto_squash_merge() {
        let args = pr_automerge_args("greenroom-robotics/ros-recipes", "release/mise");
        assert!(args.contains(&"--auto".to_string()));
        assert!(args.contains(&"--squash".to_string()));
    }

    // The rolling-PR contract: the branch name must NOT embed the version, so
    // every release of a source repo force-pushes onto the same branch and
    // updates one PR. A per-version branch leaves superseded PRs open and lets
    // an older release merge over a newer one.
    #[test]
    fn release_branch_is_version_independent() {
        let a = release_branch("mise", None);
        assert_eq!(a, "release/mise");
        assert!(!a.contains("4.5"), "branch must not embed a version: {a}");
    }

    // Multi-package repos get a per-package rolling branch so each package's
    // release lands on its own PR instead of sharing one repo-level PR.
    #[test]
    fn release_branch_is_per_package_when_package_differs() {
        assert_eq!(
            release_branch("platform_toolbox", Some("topic_utils")),
            "release/platform_toolbox/topic_utils"
        );
    }

    // Single-package repos pass --package == repo name; don't produce a
    // redundant release/mise/mise.
    #[test]
    fn release_branch_collapses_when_package_matches_repo() {
        assert_eq!(release_branch("mise", Some("mise")), "release/mise");
    }

    // Title mirrors the branch: per-package vs repo-level.
    #[test]
    fn release_title_mirrors_branch() {
        assert_eq!(
            release_title("platform_toolbox", Some("topic_utils"), "v1.26.0"),
            "release: platform_toolbox/topic_utils v1.26.0"
        );
        assert_eq!(
            release_title("mise", Some("mise"), "v4.5.2"),
            "release: mise v4.5.2"
        );
        assert_eq!(
            release_title("mise", None, "v4.5.2"),
            "release: mise v4.5.2"
        );
    }

    #[test]
    fn edit_args_refresh_pr_title() {
        let args = pr_edit_args(
            "greenroom-robotics/ros-recipes",
            "release/mise",
            "release: mise v4.5.2",
        );
        assert!(args.contains(&"--title".to_string()));
        assert!(args.contains(&"release: mise v4.5.2".to_string()));
    }
}
