use clap::Args;
use std::ffi::OsStr;
use std::path::PathBuf;

use crate::consts::{ORIGIN, PIXI_TOML, RECIPES_REPO};
use crate::gh::{self, PrRef};
use crate::git;
use crate::process;

/// Called by semantic-release's @semantic-release/exec plugin once a new version
/// has been determined. Opens/updates a PR on the recipes repo with the new pin.
#[derive(Args, Debug)]
pub struct RecipesPr {
    /// Release version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: String,
    /// owner/repo of the recipes repository.
    #[arg(long, default_value = RECIPES_REPO)]
    pub recipes_repo: String,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// Single package, used when semantic-release ran in multi-package mode.
    #[arg(long)]
    pub package: Option<String>,
    /// Accepted and ignored for compatibility: older callers (the recipes-pr
    /// composite action) still pass `--ros-distro`, but nothing reads it.
    /// `Option` rather than a defaulted `String` so passing it is
    /// distinguishable from omitting it, and thus warnable.
    #[arg(long, hide = true)]
    pub ros_distro: Option<String>,
    /// Tagged commit SHA (matches ${nextRelease.gitHead}). Used as source.rev for vendored recipes.
    #[arg(long)]
    pub sha: String,
    /// In a sweep (set by the action when no explicit package is requested), a
    /// name with no monorepo pixi.toml and no vendored recipe is a non-conda
    /// package (e.g. a launch/meta package) — skip it instead of erroring.
    /// Omitted for explicit single-package releases so a typo still fails loudly.
    #[arg(long)]
    pub allow_missing_recipe: bool,
}

impl RecipesPr {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::commands::ci::recipes_upsert;

        if self.ros_distro.is_some() {
            tracing::warn!("--ros-distro is accepted but ignored; remove it from the caller");
        }

        // 1. Resolve which package(s) we're upserting. semantic-release always
        //    invokes us with a known version; in single-package mode --package
        //    is set; in multi-package mode the plugin's cwd is the package
        //    being released, so --package may also be set there. If --package
        //    is empty we upsert every package in --package-dir at the same
        //    version (matches platform_cli's behavior). A `--package` naming
        //    something with no monorepo pixi.toml (and no root manifest) is a
        //    vendored monorepo package — its conda artifact is built from a
        //    hand-authored vendor_recipes/<name>/recipe.yaml, so there's
        //    nothing to discover.
        // Resolved once: every path-relativization below anchors on it, and
        // it is also where the source repo's remote is read from.
        let cwd = std::env::current_dir()?;

        let mode = release_target(&self.package_dir, self.package.as_deref())?;
        let targets: Vec<(String, Option<String>)> = match &mode {
            ReleaseTarget::VendoredByName(name) => vec![(name.clone(), None)],
            ReleaseTarget::Discovered => {
                let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_deref())?;
                if pkgs.is_empty() {
                    anyhow::bail!("no packages found under {}", self.package_dir.display());
                }
                let mut out = Vec::new();
                // Path from the source-repo root to the dir holding each
                // package's pixi.toml. "" or "." means the package sits at
                // the repo root. Anchor on the git toplevel, not cwd:
                // multi-semantic-release runs the publish step with cwd =
                // the package dir, and --package-dir arrives absolute, so
                // cwd-stripping would leak an absolute subdir.
                let toplevel = git::toplevel(&cwd)?;
                for pkg in &pkgs {
                    let abs = if pkg.dir.is_absolute() {
                        pkg.dir.clone()
                    } else {
                        cwd.join(&pkg.dir)
                    };
                    let parent = abs
                        .strip_prefix(&toplevel)
                        .map(|r| r.to_owned())
                        .unwrap_or(abs)
                        .to_string_lossy()
                        .into_owned();
                    let subdir = match parent.as_str() {
                        "" | "." => None,
                        s => Some(s.to_string()),
                    };
                    out.push((pkg.identity()?.name, subdir));
                }
                out
            }
        };

        // 2. Identify the source repo from the current git remote.
        let src_url = git::https_remote_url(&git::remote_url(&cwd, ORIGIN)?);
        let src_short = git::short_name(&src_url)?;
        let tag = format!("v{}", self.version);
        let run_id = std::env::var("GITHUB_RUN_ID").ok();

        // 3. Clone the recipes repo into a tempdir.
        let tmp = tempfile::TempDir::new()?;
        let recipes_root = tmp.path().join("recipes");
        git::shallow_clone(
            &gh::clone_url(&self.recipes_repo),
            crate::consts::DEFAULT_BRANCH,
            &recipes_root,
        )?;

        // 4. Create or continue the rolling release branch. The rolling PR
        //    being open is the sole signal: pending entries on an open
        //    rolling PR are never clobbered — the upsert is block-scoped per
        //    package, so appending preserves sibling entries; a closed
        //    (unmerged) PR means the content was rejected and starting fresh
        //    from main is correct. This also covers same-run coupled
        //    releases: the first package's push creates the PR, so the
        //    second package sees it open and appends.
        let branch = release_branch(&src_short);
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

        // 5. Apply each package's release (vendored recipe or rosdistro upsert).
        use std::collections::BTreeSet;
        let mut changed: BTreeSet<std::path::PathBuf> = BTreeSet::new();
        // Package -> tag for everything the PR carries. Seeded from the open
        // PR's body so a rolling PR that already holds siblings keeps listing
        // them (at their own versions) instead of being rewritten around
        // whichever package released last.
        let mut released: std::collections::BTreeMap<String, String> = existing_body
            .as_deref()
            .map(body_packages)
            .unwrap_or_default();
        // The refs each package was pinned to before this release, for a diff link.
        let mut old_refs: Vec<recipes_upsert::OldRef> = Vec::new();
        for (name, subdir) in &targets {
            // A vendored-by-name target with no recipe would otherwise fall
            // through apply_release to a spurious pixi-native entry. Decide
            // skip-vs-error based on whether this is a tolerant sweep.
            let has_recipe = recipes_upsert::vendored_recipe_path(&recipes_root, name).is_some();
            match recipe_action(&mode, has_recipe, self.allow_missing_recipe) {
                RecipeAction::Skip => {
                    println!("skipping {name}: no monorepo pixi.toml and no vendored recipe");
                    continue;
                }
                RecipeAction::Error => anyhow::bail!(
                    "package {name} has no monorepo pixi.toml and no vendored recipe \
                     (vendor_recipes/{name}/recipe.yaml or its hyphenated form) in {}",
                    self.recipes_repo
                ),
                RecipeAction::Apply => {}
            }
            let applied = recipes_upsert::apply_release(
                &recipes_root,
                name,
                &src_url,
                &tag,
                &self.version,
                &self.sha,
                subdir.as_deref(),
            )?;
            changed.insert(applied.path);
            old_refs.extend(applied.old_ref);
            released.insert(name.clone(), tag.clone());
        }

        // Every target was skipped (sweep tolerating packages with no conda
        // recipe) — there's nothing to commit, so don't try to open a PR.
        if changed.is_empty() {
            println!("no recipe changes to publish; nothing to do");
            return Ok(());
        }

        // 6. Commit + push + open PR.
        let add_args: Vec<&OsStr> = std::iter::once(OsStr::new("add"))
            .chain(changed.iter().map(|p| p.as_os_str()))
            .collect();
        process::run_in(&recipes_root, "git", &add_args)?;
        let title = release_title(&src_short, &released, &tag);
        // A package can be a no-op against the recipes checkout: an earlier CI
        // run (e.g. a sibling release workflow sweeping the same tag) may have
        // already staged the identical recipe content, in which case `git
        // diff --cached --quiet` reports nothing staged and `git commit`
        // would fail with "nothing to commit, working tree clean". What to do
        // about that depends on the baseline: if we reset from main (no open
        // PR), there's nothing new to publish at all, so bail out before the
        // push/PR steps rather than force-pushing main-as-is and opening an
        // empty PR. If we appended to an open PR, the branch carries prior
        // pending content that still needs to reach the remote, so we skip
        // only the commit and continue through push + PR refresh.
        match classify_noop(git::nothing_staged(&recipes_root)?, pr_open) {
            NoopOutcome::EarlyReturn => {
                println!("recipe for {title} already up to date; nothing to publish");
                return Ok(());
            }
            NoopOutcome::SkipCommitKeepPush => {
                tracing::info!("recipe for {title} already up to date; skipping commit");
            }
            NoopOutcome::Commit => {
                // Commit as the App bot. The release action exports its identity via
                // GIT_AUTHOR_*/GIT_COMMITTER_*, which git honours natively; we mirror
                // it onto -c so a standalone run still has a usable identity, falling
                // back to the greenroom-bot label only when the env is unset.
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
        // Plain --force, not --force-with-lease: the recipes repo is cloned
        // shallow on `main` only, so there's no remote-tracking ref for the
        // rolling branch and --force-with-lease would reject the push.
        process::run_in(&recipes_root, "git", &["push", "--force", ORIGIN, &branch])?;

        // Link to the source-repo diff between what the recipe was pinned to
        // before and this release, so a reviewer sees what changed.
        let old = diff_ref(&old_refs, &self.sha);
        let body = pr_body(
            old.map(|o| compare_url(&src_url, o, &self.sha)).as_deref(),
            &released,
        );

        if pr_open {
            // The rolling PR already exists from a previous release; refresh its
            // title and body so the version and diff link aren't stale.
            gh::pr::edit(pr, &title, &body)?;
            println!("PR already exists for {branch}; branch, title and body updated.");
        } else {
            gh::pr::create(pr, &title, &body)?;
        }

        // Enable GitHub native auto-merge so the PR lands once CI passes
        // (mirrors `mise bump`'s behavior).
        gh::pr::merge_auto(pr)?;

        // Drop a link to the recipes PR into the Actions run summary so the
        // release job's page points straight at it. Best-effort: a missing URL
        // or no $GITHUB_STEP_SUMMARY (local run) must not fail the release.
        if let Some(url) = gh::pr::view_url(pr) {
            println!("recipes PR: {url}");
            gh::summary::append(&format!("### Recipes PR\n\n[{title}]({url})\n"));
        }

        Ok(())
    }
}

/// Version-independent rolling branch, one per source repo. Packages land on
/// it as long as the rolling PR is open (see `pr_exists`); a closed/absent PR
/// resets it from main. Shared so coupled sibling releases land in ONE
/// recipes PR and build together in one pr-validate run.
fn release_branch(src_short: &str) -> String {
    format!("release/{src_short}")
}

fn run_marker(run_id: &str) -> String {
    format!("[mise-run:{run_id}]")
}

/// PR title / commit message, mirroring `release_branch`. Single-package repos
/// read `release: <repo> v<ver>`; one package `release: <repo>/<pkg> v<ver>`;
/// several `release: <repo>/{a v1.2.3, b v0.4.0}`. Packages on a rolling PR are
/// each at their own version, so the version travels with the name. A list too
/// long for GitHub's 256-char title collapses to a count — the full list lives
/// in the PR body, which is what the next append reads back.
fn release_title(
    src_short: &str,
    packages: &std::collections::BTreeMap<String, String>,
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
/// keep it inside the conventional 72-char subject line rather than GitHub's
/// 256-char title cap.
const MAX_TITLE_CHARS: usize = 72;

/// How `recipes-pr` sources the package(s) to release.
#[derive(Debug, PartialEq, Eq)]
enum ReleaseTarget {
    /// Discover pixi packages under `--package-dir` (existing behavior).
    Discovered,
    /// A single package named by `--package` that has no monorepo pixi.toml;
    /// its conda artifact is built from a hand-authored vendored recipe
    /// (e.g. deepstream_extensions).
    VendoredByName(String),
}

/// True when a manifest exists at `path` AND declares a `[package]`.
///
/// The distinction matters: a *workspace-only* manifest is a dev environment
/// for a package built somewhere else, so for release purposes it is the same
/// as having no manifest at all. Checking mere file existence would classify
/// such a package as `Discovered` and then fail on the missing `[package]`.
fn declares_package_at(path: &std::path::Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(matches!(
        crate::manifest::Manifest::read(path)?,
        crate::manifest::Manifest::Package(_)
    ))
}

/// Choose the release target. A `--package` with neither a per-package
/// `<dir>/<pkg>/pixi.toml` nor a root `<dir>/pixi.toml` *declaring a package*
/// is a vendored monorepo package; everything else discovers as before.
fn release_target(
    package_dir: &std::path::Path,
    package: Option<&str>,
) -> anyhow::Result<ReleaseTarget> {
    match package {
        Some(pkg)
            if !declares_package_at(&package_dir.join(pkg).join(PIXI_TOML))?
                && !declares_package_at(&package_dir.join(PIXI_TOML))? =>
        {
            Ok(ReleaseTarget::VendoredByName(pkg.to_string()))
        }
        _ => Ok(ReleaseTarget::Discovered),
    }
}

/// What to do with a resolved target once we know whether a vendored recipe
/// exists for it. Only a `VendoredByName` with no recipe is special: skip it in
/// a tolerant sweep (`allow_missing`), else fail loudly (explicit target).
#[derive(Debug, PartialEq, Eq)]
enum RecipeAction {
    Apply,
    Skip,
    Error,
}

fn recipe_action(mode: &ReleaseTarget, has_recipe: bool, allow_missing: bool) -> RecipeAction {
    match mode {
        ReleaseTarget::VendoredByName(_) if !has_recipe => {
            if allow_missing {
                RecipeAction::Skip
            } else {
                RecipeAction::Error
            }
        }
        _ => RecipeAction::Apply,
    }
}

/// GitHub compare URL between two refs of the source repo.
fn compare_url(src_url: &str, old: &str, new: &str) -> String {
    format!("{}/compare/{old}...{new}", src_url.trim_end_matches(".git"))
}

/// The old ref to diff against `sha`. There's one pin per package (normally a
/// single package); prefer an immutable rev over a mutable tag. `None` for a
/// brand-new package (no prior pin) or a same-rev re-pin (nothing to show).
fn diff_ref<'a>(
    old_refs: &'a [crate::commands::ci::recipes_upsert::OldRef],
    sha: &str,
) -> Option<&'a str> {
    old_refs
        .iter()
        .find(|r| r.is_rev())
        .or_else(|| old_refs.first())
        .map(|r| r.value())
        .filter(|v| *v != sha)
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

/// The body's package list, the authoritative record of what a rolling PR
/// carries: the title is display-only and gets shortened once it has too many
/// packages to fit, so it can't be read back. Inverse of the `- <pkg> <tag>`
/// lines `pr_body` writes.
fn body_packages(body: &str) -> std::collections::BTreeMap<String, String> {
    // ponytail: any `- <word> <word>` bullet in the body reads as a package, so
    // a hand-added bullet can put a bogus name in the title (nothing else). Use
    // a fenced/HTML-comment block if bodies ever grow other bullet lists.
    body.lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .filter_map(|entry| entry.rsplit_once(' '))
        .map(|(name, tag)| (name.to_string(), tag.to_string()))
        .collect()
}

fn pr_body(diff: Option<&str>, packages: &std::collections::BTreeMap<String, String>) -> String {
    // ponytail: nanos-modulo pick, no rng dep needed for flavor text
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0)
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

/// The three things that can happen with a staged upsert, depending on
/// whether anything is actually staged and whether the rolling PR is open
/// (i.e. whether the branch was appended-to or reset from main).
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

fn classify_noop(nothing_staged: bool, pr_open: bool) -> NoopOutcome {
    match (nothing_staged, pr_open) {
        (false, _) => NoopOutcome::Commit,
        (true, true) => NoopOutcome::SkipCommitKeepPush,
        (true, false) => NoopOutcome::EarlyReturn,
    }
}

#[cfg(test)]
#[path = "recipes_pr_tests.rs"]
mod tests;
