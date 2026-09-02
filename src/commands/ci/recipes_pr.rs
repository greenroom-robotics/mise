use clap::Args;
use std::ffi::OsStr;
use std::path::PathBuf;

use crate::consts::{ORIGIN, PIXI_TOML, RECIPES_REPO};
use crate::gh::{self, PrRef};
use crate::git;
use crate::process;
use crate::types::{GithubRepoUrl, PackageName, Sha40, Version};

/// Opens/updates a PR on the recipes repo pinning the released version.
#[derive(Args, Debug)]
pub struct RecipesPr {
    /// Release version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: Version,
    /// owner/repo of the recipes repository.
    #[arg(long, default_value = RECIPES_REPO)]
    pub recipes_repo: String,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// Single package, used when semantic-release ran in multi-package mode.
    #[arg(long)]
    pub package: Option<PackageName>,
    /// Ignored legacy flag; passing it logs a warning.
    #[arg(long, hide = true)]
    pub ros_distro: Option<String>,
    /// Tagged commit SHA (matches ${nextRelease.gitHead}). Used as source.rev for vendored recipes.
    #[arg(long)]
    pub sha: Sha40,
    /// In a sweep, a name with no monorepo pixi.toml and no vendored recipe is
    /// a non-conda package (e.g. a launch/meta package) — skip it instead of
    /// erroring. Omit for explicit single-package releases so a typo fails loudly.
    #[arg(long)]
    pub allow_missing_recipe: bool,
    /// Repeatable. Packages whose pixi-native entry gets `lfs: true`, so
    /// `mise build-recipes pixi` pulls their LFS objects before building.
    /// Authoritative: a package released without being listed has any existing
    /// `lfs: true` removed.
    #[arg(long = "lfs-package")]
    pub lfs_packages: Vec<PackageName>,
}

impl RecipesPr {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        use crate::commands::ci::recipes_upsert;

        if self.ros_distro.is_some() {
            tracing::warn!("--ros-distro is accepted but ignored; remove it from the caller");
        }

        let cwd = std::env::current_dir()?;

        let mode = release_mode(&self.package_dir, self.package.as_ref())?;
        let targets: Vec<(PackageName, Option<String>)> = match &mode {
            ReleaseMode::VendoredByName(name) => vec![(name.clone(), None)],
            ReleaseMode::Discovered => {
                let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
                if pkgs.is_empty() {
                    color_eyre::eyre::bail!(
                        "no packages found under {}",
                        self.package_dir.display()
                    );
                }
                // Subdir from the source-repo root to each package's
                // pixi.toml ("" or "." = repo root). Anchored on the git
                // toplevel: the publish step's cwd is the package dir and
                // --package-dir arrives absolute, so cwd-stripping would
                // leak an absolute subdir.
                let toplevel = git::toplevel(&cwd)?;
                pkgs.iter()
                    .map(|pkg| {
                        let abs = if pkg.dir.is_absolute() {
                            pkg.dir.clone()
                        } else {
                            cwd.join(&pkg.dir)
                        };
                        let parent = abs
                            .strip_prefix(&toplevel)
                            .map(std::borrow::ToOwned::to_owned)
                            .unwrap_or(abs)
                            .to_string_lossy()
                            .into_owned();
                        let subdir = match parent.as_str() {
                            "" | "." => None,
                            s => Some(s.to_string()),
                        };
                        (pkg.identity().name, subdir)
                    })
                    .collect()
            }
        };

        let src_url = GithubRepoUrl::parse_remote(&git::remote_url(&cwd, ORIGIN)?)?;
        let src_short = src_url.repo();
        let tag = format!("v{}", self.version);
        let run_id = std::env::var("GITHUB_RUN_ID").ok();

        let tmp = tempfile::TempDir::new()?;
        let recipes_root = tmp.path().join("recipes");
        git::shallow_clone(
            &gh::clone_url(&self.recipes_repo),
            crate::consts::DEFAULT_BRANCH,
            &recipes_root,
        )?;

        // The rolling PR being open is the sole signal: open → append (the
        // upsert is block-scoped per package, so sibling entries survive);
        // closed/absent → its content was rejected, reset from main.
        let branch = release_branch(src_short);
        let pr = PrRef::new(&self.recipes_repo, &branch)?;
        let existing_body = gh::pr::list_body(pr);
        let pr_open = existing_body.is_some();
        if pr_open {
            git::fetch_branch(&recipes_root, &branch)?;
            process::run_in(
                &recipes_root,
                "git",
                &["checkout", "-b", &branch, "FETCH_HEAD"],
            )?;
        } else {
            process::run_in(&recipes_root, "git", &["checkout", "-b", &branch])?;
        }

        use std::collections::BTreeSet;
        let mut changed: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        // Seeded from the open PR's body so siblings already on the rolling
        // PR keep their entries at their own versions.
        let mut released: std::collections::BTreeMap<PackageName, String> = existing_body
            .as_deref()
            .map(body_packages)
            .unwrap_or_default();
        let mut old_refs: Vec<recipes_upsert::OldRef> = Vec::new();
        for (name, subdir) in &targets {
            // A vendored-by-name target with no recipe would otherwise fall
            // through routing to a spurious pixi-native entry.
            let has_recipe = recipes_upsert::vendored_recipe_path(&recipes_root, name).is_some();
            match recipe_action(&mode, has_recipe, self.allow_missing_recipe) {
                RecipeAction::Skip => {
                    println!("skipping {name}: no monorepo pixi.toml and no vendored recipe");
                    continue;
                }
                RecipeAction::Error => color_eyre::eyre::bail!(
                    "package {name} has no monorepo pixi.toml and no vendored recipe \
                     (vendor_recipes/{name}/recipe.yaml or its hyphenated form) in {}",
                    self.recipes_repo
                ),
                RecipeAction::Apply => {}
            }
            let target = recipes_upsert::route(
                &recipes_root,
                name,
                &src_url,
                &tag,
                &self.version,
                &self.sha,
                recipes_upsert::PixiEntryOpts {
                    subdir: subdir.as_deref(),
                    lfs: self.lfs_packages.contains(name),
                },
            )?;
            changed.insert(target.rel_path());
            old_refs.extend(recipes_upsert::apply(&recipes_root, &target)?);
            released.insert(name.clone(), tag.clone());
        }

        if changed.is_empty() {
            println!("no recipe changes to publish; nothing to do");
            return Ok(());
        }

        let add_args: Vec<&OsStr> = std::iter::once(OsStr::new("add"))
            .chain(changed.iter().map(|p| p.as_os_str()))
            .collect();
        process::run_in(&recipes_root, "git", &add_args)?;
        let title = release_title(src_short, &released, &tag);
        match classify_noop(git::nothing_staged(&recipes_root)?, pr_open) {
            NoopOutcome::EarlyReturn => {
                println!("recipe for {title} already up to date; nothing to publish");
                return Ok(());
            }
            NoopOutcome::SkipCommitKeepPush => {
                tracing::info!("recipe for {title} already up to date; skipping commit");
            }
            NoopOutcome::Commit => {
                let (git_name, git_email) = git_identity();
                let name_cfg = format!("user.name={git_name}");
                let email_cfg = format!("user.email={git_email}");
                let mut commit_msg = title.clone();
                if let Some(id) = &run_id {
                    commit_msg.push_str(&format!("\n\n{}", run_marker(id)));
                }
                process::run_in(
                    &recipes_root,
                    "git",
                    &[
                        "-c",
                        &name_cfg,
                        "-c",
                        &email_cfg,
                        "commit",
                        "-m",
                        &commit_msg,
                    ],
                )?;
            }
        }
        // The clone is shallow on `main` only: the rolling branch has no
        // remote-tracking ref, so --force-with-lease would reject the push.
        process::run_in(&recipes_root, "git", &["push", "--force", ORIGIN, &branch])?;

        let old = diff_ref(&old_refs, &self.sha);
        let body = pr_body(
            old.map(|o| src_url.compare_url(o, self.sha.as_str()))
                .as_deref(),
            &released,
        );

        if pr_open {
            gh::pr::edit(pr, &title, &body)?;
            println!("PR already exists for {branch}; branch, title and body updated.");
        } else {
            gh::pr::create(pr, &title, &body)?;
        }

        gh::pr::merge_auto(pr)?;

        // Best-effort: a missing URL or no $GITHUB_STEP_SUMMARY (local run)
        // must not fail the release.
        if let Some(url) = gh::pr::view_url(pr) {
            println!("recipes PR: {url}");
            gh::summary::append(&format!("### Recipes PR\n\n[{title}]({url})\n"));
        }

        Ok(())
    }
}

/// Version-independent rolling branch, one per source repo, so coupled
/// sibling releases land in one recipes PR and build together.
fn release_branch(src_short: &str) -> String {
    format!("release/{src_short}")
}

fn run_marker(run_id: &str) -> String {
    format!("[mise-run:{run_id}]")
}

/// PR title / commit message: `release: <repo> v1`, `release: <repo>/<pkg> v1`,
/// or `release: <repo>/{a v1, b v2}`. A list too long for the title collapses
/// to a count — the full list lives in the PR body, which is what the next
/// append reads back.
fn release_title(
    src_short: &str,
    packages: &std::collections::BTreeMap<PackageName, String>,
    tag: &str,
) -> String {
    let pkgs: Vec<(&str, &str)> = packages
        .iter()
        .filter(|(name, _)| name.as_str() != src_short)
        .map(|(name, tag)| (name.as_str(), tag.as_str()))
        .collect();
    let title = match pkgs.as_slice() {
        [] => format!("release: {src_short} {tag}"),
        [(name, tag)] => format!("release: {src_short}/{name} {tag}"),
        many => {
            let list: Vec<String> = many.iter().map(|(n, t)| format!("{n} {t}")).collect();
            format!("release: {src_short}/{{{}}}", list.join(", "))
        }
    };
    if title.chars().count() > MAX_TITLE_CHARS {
        return format!("release: {src_short} ({} packages)", pkgs.len());
    }
    title
}

/// Recipes PRs are squash-merged, so the title becomes the commit subject —
/// keep it inside the conventional 72-char subject line.
const MAX_TITLE_CHARS: usize = 72;

/// Where the list of packages to release comes from — not where a release
/// lands (that is [`recipes_upsert::ReleaseTarget`]).
#[derive(Debug, PartialEq, Eq)]
enum ReleaseMode {
    /// Discover pixi packages under `--package-dir`.
    Discovered,
    /// A single package named by `--package` that has no monorepo pixi.toml;
    /// its conda artifact is built from a hand-authored vendored recipe.
    VendoredByName(PackageName),
}

/// True when a manifest exists at `path` AND declares a `[package]`. A
/// workspace-only manifest is a dev environment for a package built somewhere
/// else, so for release purposes it is the same as no manifest at all.
fn declares_package_at(path: &std::path::Path) -> color_eyre::eyre::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(matches!(
        crate::manifest::Manifest::read(path)?,
        crate::manifest::Manifest::Package(_)
    ))
}

/// A `--package` with neither a per-package `<dir>/<pkg>/pixi.toml` nor a
/// root `<dir>/pixi.toml` declaring a package is a vendored monorepo package;
/// everything else is `Discovered`.
fn release_mode(
    package_dir: &std::path::Path,
    package: Option<&PackageName>,
) -> color_eyre::eyre::Result<ReleaseMode> {
    match package {
        Some(pkg)
            if !declares_package_at(&package_dir.join(pkg.as_str()).join(PIXI_TOML))?
                && !declares_package_at(&package_dir.join(PIXI_TOML))? =>
        {
            Ok(ReleaseMode::VendoredByName(pkg.clone()))
        }
        _ => Ok(ReleaseMode::Discovered),
    }
}

/// Only a `VendoredByName` with no recipe is special: skip it in a tolerant
/// sweep (`allow_missing`), else fail loudly.
#[derive(Debug, PartialEq, Eq)]
enum RecipeAction {
    Apply,
    Skip,
    Error,
}

const fn recipe_action(mode: &ReleaseMode, has_recipe: bool, allow_missing: bool) -> RecipeAction {
    match mode {
        ReleaseMode::VendoredByName(_) if !has_recipe => {
            if allow_missing {
                RecipeAction::Skip
            } else {
                RecipeAction::Error
            }
        }
        _ => RecipeAction::Apply,
    }
}

/// The old ref to diff against `sha`. There's one pin per package (normally a
/// single package); prefer an immutable rev over a mutable tag. `None` for a
/// brand-new package (no prior pin) or a same-rev re-pin (nothing to show).
fn diff_ref<'a>(
    old_refs: &'a [crate::commands::ci::recipes_upsert::OldRef],
    sha: &Sha40,
) -> Option<&'a str> {
    old_refs
        .iter()
        .find(|r| r.is_rev())
        .or_else(|| old_refs.first())
        .map(super::recipes_upsert::OldRef::value)
        .filter(|v| *v != sha.as_str())
}

/// Commit identity: `GIT_AUTHOR_NAME/EMAIL` when set, else the greenroom-bot
/// label so a standalone run still has a usable identity.
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

/// Gremlin flavor text for PR bodies.
const GREMLINS: &[&str] = &[
    "🐉 A gremlin smelled a fresh release and dragged this recipe in by its tail.",
    "🐉 The recipe gremlins have been fed. They demand you click merge.",
    "🐉 *gremlin noises* — new version spotted, recipe updated, snacks expected.",
    "🐉 A wild recipe gremlin appeared and bumped the version while you weren't looking.",
    "🐉 Do not feed the gremlins after midnight. They already shipped this PR anyway.",
    "🐉 The gremlin in the build closet insists this recipe is ready. Trust the gremlin.",
];

/// The body's package list, the authoritative record of what a rolling PR
/// carries: the title is display-only and gets shortened once it has too many
/// packages to fit, so it can't be read back. Inverse of the `- <pkg> <tag>`
/// lines `pr_body` writes.
fn body_packages(body: &str) -> std::collections::BTreeMap<PackageName, String> {
    // ponytail: any `- <word> <word>` bullet whose first word parses as a
    // package name reads as a package, so a hand-added bullet can still put a
    // bogus name in the title (nothing else). Use a fenced/HTML-comment block
    // if bodies ever grow other bullet lists.
    body.lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .filter_map(|entry| entry.rsplit_once(' '))
        .filter_map(|(name, tag)| Some((PackageName::new(name).ok()?, tag.to_string())))
        .collect()
}

fn pr_body(
    diff: Option<&str>,
    packages: &std::collections::BTreeMap<PackageName, String>,
) -> String {
    // ponytail: nanos-modulo pick, no rng dep needed for flavor text
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as usize)
        % GREMLINS.len();
    let mut body = GREMLINS[idx].to_string();
    body.push_str("\n\n**Releasing:**\n");
    for (name, tag) in packages {
        body.push_str(&format!("- {name} {tag}\n"));
    }
    if let Some(url) = diff {
        body.push_str(&format!("\n\n**Diff since last release:** {url}"));
    }
    body.push_str("\n\nAutomated by `mise ci recipes-pr`.");
    body
}

#[derive(Debug, PartialEq, Eq)]
enum NoopOutcome {
    /// Something is staged: commit it normally.
    Commit,
    /// Nothing staged, but we appended to an open PR: the branch still
    /// carries prior pending content, so skip the commit but keep pushing
    /// and refreshing the PR.
    SkipCommitKeepPush,
    /// Nothing staged and we reset from main (no open PR): there is nothing
    /// new to publish at all — return before push/PR steps.
    EarlyReturn,
}

const fn classify_noop(nothing_staged: bool, pr_open: bool) -> NoopOutcome {
    match (nothing_staged, pr_open) {
        (false, _) => NoopOutcome::Commit,
        (true, true) => NoopOutcome::SkipCommitKeepPush,
        (true, false) => NoopOutcome::EarlyReturn,
    }
}

#[cfg(test)]
#[path = "recipes_pr_tests.rs"]
mod tests;
