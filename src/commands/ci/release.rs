use clap::Args;
use color_eyre::eyre::WrapErr;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::manifest::Package;
use crate::types::{PackageName, Version};

const RECORD_PLUGIN: &str = include_str!("record_release.js");

/// Which semantic-release pass a `.releaserc` is for.
enum Pass<'a> {
    /// `--dry-run`: the record plugin writes each package's next version and
    /// notes under `dir`; nothing is committed or tagged.
    Record { dir: &'a Path },
    /// The real run. `changelog_committed` is set when a prior release commit
    /// already prepended CHANGELOG.md, so the changelog plugin must not run
    /// again and dirty the tree.
    Release { changelog_committed: bool },
}

/// What the record plugin wrote for one package in the dry-run pass.
#[derive(Debug, Clone, serde::Deserialize)]
struct Recorded {
    version: Version,
    notes: String,
}

/// Prepend `notes` to a CHANGELOG.md the way @semantic-release/changelog does.
fn prepend_changelog(existing: &str, notes: &str) -> String {
    let existing = existing.trim();
    if existing.is_empty() {
        format!("{}\n", notes.trim())
    } else {
        format!("{}\n\n{existing}\n", notes.trim())
    }
}

fn release_commit_message(releases: &[(&PackageName, &Recorded)]) -> String {
    let subject = releases
        .iter()
        .map(|(name, rel)| format!("{name} {}", rel.version))
        .collect::<Vec<_>>()
        .join(", ");
    let body = releases
        .iter()
        .map(|(_, rel)| rel.notes.trim())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("chore(release): {subject} [skip ci]\n\n{body}")
}

#[derive(Args, Debug)]
pub struct Release {
    /// Single package to release (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<PackageName>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// owner/repo of the conda recipes repository to upsert into.
    #[arg(long, default_value = crate::consts::RECIPES_REPO)]
    pub recipes_repo: String,
    /// Whether to commit CHANGELOG.md back to the source repo.
    // ArgAction::Set so `--changelog true|false` both parse.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub changelog: bool,
    /// Comma-separated branch list passed to semantic-release.
    #[arg(long, default_value = "main,master,alpha")]
    pub release_branches: String,
    /// Whether to create a GitHub release.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub github_release: bool,
    /// Extra path(s) to include in the release commit alongside pixi.toml,
    /// committed and tagged in the same `chore(release)` commit. Repeatable.
    #[arg(long)]
    pub extra_git_asset: Vec<String>,
    /// Extra shell command appended (with `&&`) to semantic-release's prepare
    /// step, run after the pixi bump and before the release commit. The
    /// `${nextRelease.version}` placeholder is available.
    #[arg(long)]
    pub extra_prepare_cmd: Option<String>,
}

/// semantic-release tag format. Both modes tag `<package>@<version>` — in
/// multi-package mode multi-semantic-release substitutes `${name}` itself; in
/// single-package mode the resolved package name is embedded literally.
fn tag_format(multi: bool, single_pkg_name: &PackageName) -> String {
    if multi {
        "${name}@${version}".to_string()
    } else {
        format!("{single_pkg_name}@${{version}}")
    }
}

/// Sibling deps that msr should **order** on for `name`: path deps only. These
/// are encoded as `"*"` in the synthesized package.json, which (with
/// `--deps.release=inherit`, see `release_argv`) orders a coupled release
/// sibling-first WITHOUT triggering a dependency-only cascade. Committed `==`
/// pins are excluded — a released consumer is decoupled and needs no ordering.
fn msr_ordering_deps(
    graph: &crate::commands::ci::siblings::SiblingGraph,
    name: &PackageName,
) -> BTreeSet<PackageName> {
    graph.path_deps.get(name).cloned().unwrap_or_default()
}

/// `npx` argv for the release. Multi-package mode adds `--deps.release=inherit`:
/// with the synthesized package.json ranges pinned to `"*"`, a sibling release
/// always satisfies the range, so multi-semantic-release orders the coupled
/// release sibling-first but never cascade-releases a consumer that has no
/// commits of its own.
fn release_argv(multi: bool, tag_format: &str) -> Vec<String> {
    let bin = if multi {
        "multi-semantic-release"
    } else {
        "semantic-release"
    };
    let mut argv = vec![
        "--no-install".to_string(),
        bin.to_string(),
        format!("--tag-format={tag_format}"),
    ];
    if multi {
        argv.push("--deps.release=inherit".to_string());
    }
    argv
}

/// Per-workspace package.json synthesized at release time so the patched
/// multi-semantic-release discovers packages and releases them in topological
/// order of sibling deps. Never committed (repos don't track package.json).
///
/// Deliberately NOT `private: true`: msr's default `ignorePrivate` skips
/// private workspace packages entirely (observed as "Queued 0 packages").
/// Nothing npm-publishes these — the .releaserc has no @semantic-release/npm.
fn package_json_for(
    name: &PackageName,
    version: &Version,
    deps: &BTreeSet<PackageName>,
) -> serde_json::Result<String> {
    let deps_obj: serde_json::Map<String, serde_json::Value> = deps
        .iter()
        .map(|d| (d.to_string(), serde_json::Value::String("*".into())))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": name,
        "version": version.to_string(),
        "dependencies": deps_obj,
    }))
}

/// Absolutize a path against cwd. multi-semantic-release runs each package's
/// semantic-release — and therefore the exec plugin's shell commands — with
/// cwd = the package directory, so paths embedded in prepareCmd/publishCmd
/// must be absolute to survive.
fn absolute(p: &std::path::Path) -> std::path::PathBuf {
    if p.is_absolute() {
        p.to_owned()
    } else {
        std::env::current_dir().map_or_else(|_| p.to_owned(), |cwd| cwd.join(p))
    }
}

/// Convert an absolute path to a relative path from cwd if possible;
/// otherwise return the path unchanged. Workspace globs in package.json
/// must be cwd-relative for npm/yarn/msr discovery to work correctly.
fn cwd_relative(p: &std::path::Path) -> std::path::PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| p.strip_prefix(&cwd).ok())
        .map_or_else(|| p.to_owned(), std::borrow::ToOwned::to_owned)
}

/// Branch the release commit is pushed to. CI checks out a detached SHA, so
/// `HEAD` alone is not a pushable ref there; `GITHUB_REF_NAME` names the branch.
fn release_branch() -> color_eyre::eyre::Result<String> {
    let branch = match std::env::var("GITHUB_REF_NAME") {
        Ok(b) if !b.is_empty() => b,
        _ => crate::process::capture_in(
            Path::new("."),
            "git",
            &["rev-parse", "--abbrev-ref", "HEAD"],
        )?
        .trim()
        .to_string(),
    };
    if branch == "HEAD" {
        color_eyre::eyre::bail!(
            "detached HEAD and GITHUB_REF_NAME unset: cannot tell which branch to push to"
        );
    }
    Ok(branch)
}

/// Packages the dry-run pass recorded a next release for, in `pkgs` order.
fn read_recorded<'p>(
    dir: &Path,
    pkgs: &'p [Package],
) -> color_eyre::eyre::Result<Vec<(&'p Package, Recorded)>> {
    let mut out = Vec::new();
    for pkg in pkgs {
        let path = dir.join(format!("{}.json", pkg.identity().name));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rec: Recorded =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        out.push((pkg, rec));
    }
    Ok(out)
}

/// Merge a `workspaces` array into the root package.json, creating a minimal
/// one if absent.
fn ensure_root_workspaces(
    root_pkg_json: &std::path::Path,
    globs: &[String],
) -> color_eyre::eyre::Result<()> {
    use color_eyre::eyre::{ContextCompat, WrapErr};
    let mut v: serde_json::Value = if root_pkg_json.exists() {
        let text = std::fs::read_to_string(root_pkg_json)
            .with_context(|| format!("reading {}", root_pkg_json.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", root_pkg_json.display()))?
    } else {
        serde_json::json!({ "name": "mise-release-root", "private": true })
    };
    v.as_object_mut()
        .with_context(|| format!("{} is not a JSON object", root_pkg_json.display()))?
        .insert("workspaces".to_string(), serde_json::json!(globs));
    std::fs::write(root_pkg_json, serde_json::to_string_pretty(&v)?)
        .with_context(|| format!("writing {}", root_pkg_json.display()))?;
    Ok(())
}

impl Release {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
        let Some(first) = pkgs.first() else {
            color_eyre::eyre::bail!("no packages found under {}", self.package_dir.display());
        };
        let multi = self.package.is_none() && pkgs.len() > 1;

        // Multi mode ignores the name; in single mode `first` is the only package.
        let tag_format = tag_format(multi, &first.identity().name);
        let argv = release_argv(multi, &tag_format);

        if !multi {
            let pass = Pass::Release {
                changelog_committed: false,
            };
            self.write_releasercs(&pkgs, &pass)?;
            // Plain semantic-release resolves its config from cwd — a .releaserc
            // down in the package dir is invisible to it, and its own defaults
            // have no `main` branch (ERELEASEBRANCHES).
            let releaserc =
                self.releaserc_json(&first.manifest_path, &first.identity().name, &pass)?;
            std::fs::write(".releaserc", releaserc)?;
            return crate::process::run("npx", &argv);
        }

        let graph = crate::commands::ci::siblings::analyze(&pkgs);
        let mut workspace_globs: Vec<String> = Vec::new();
        for pkg in &pkgs {
            let id = pkg.identity();
            let deps = msr_ordering_deps(&graph, &id.name);
            std::fs::write(
                pkg.dir.join("package.json"),
                package_json_for(&id.name, &id.version, &deps)?,
            )?;
            workspace_globs.push(cwd_relative(&pkg.dir).to_string_lossy().into_owned());
        }
        ensure_root_workspaces(Path::new("package.json"), &workspace_globs)?;

        // One release commit for every package released this run, made before
        // any tag exists so all tags land on it. semantic-release only ever
        // sees a clean tree afterwards: bump-pixi rewrites the same version and
        // @semantic-release/git commits nothing when no asset changed.
        let record_dir = tempfile::tempdir()?;
        std::fs::write(record_dir.path().join("record_release.js"), RECORD_PLUGIN)?;
        self.write_releasercs(
            &pkgs,
            &Pass::Record {
                dir: record_dir.path(),
            },
        )?;
        let mut dry_argv = argv.clone();
        dry_argv.push("--dry-run".to_string());
        crate::process::run("npx", &dry_argv)?;
        let releases = read_recorded(record_dir.path(), &pkgs)?;
        if !releases.is_empty() {
            self.commit_release(&releases)?;
        }

        // ponytail: --extra-git-asset / --extra-prepare-cmd still dirty the
        // tree per package in this pass and so still commit per package.
        self.write_releasercs(
            &pkgs,
            &Pass::Release {
                changelog_committed: self.changelog,
            },
        )?;
        crate::process::run("npx", &argv)
    }

    fn write_releasercs(&self, pkgs: &[Package], pass: &Pass) -> color_eyre::eyre::Result<()> {
        for pkg in pkgs {
            let releaserc = self.releaserc_json(&pkg.manifest_path, &pkg.identity().name, pass)?;
            std::fs::write(pkg.dir.join(".releaserc"), releaserc)?;
        }
        Ok(())
    }

    fn commit_release(&self, releases: &[(&Package, Recorded)]) -> color_eyre::eyre::Result<()> {
        let mut files: Vec<PathBuf> = Vec::new();
        for (pkg, rel) in releases {
            let body = std::fs::read_to_string(&pkg.manifest_path)
                .with_context(|| format!("reading {}", pkg.manifest_path.display()))?;
            let bumped = crate::manifest::set_package_version(&body, &rel.version)
                .with_context(|| format!("bumping {}", pkg.manifest_path.display()))?;
            std::fs::write(&pkg.manifest_path, bumped)?;
            files.push(pkg.manifest_path.clone());
            if self.changelog {
                let changelog = pkg.dir.join("CHANGELOG.md");
                let existing = std::fs::read_to_string(&changelog).unwrap_or_default();
                std::fs::write(&changelog, prepend_changelog(&existing, &rel.notes))?;
                files.push(changelog);
            }
        }
        let names: Vec<PackageName> = releases.iter().map(|(p, _)| p.identity().name).collect();
        let subjects: Vec<(&PackageName, &Recorded)> =
            names.iter().zip(releases.iter().map(|(_, r)| r)).collect();
        let message = release_commit_message(&subjects);

        let mut add = vec!["add".to_string(), "--".to_string()];
        add.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
        crate::process::git(&add)?;
        crate::process::git(&["commit", "--quiet", "-m", &message])?;
        let refspec = format!("HEAD:refs/heads/{}", release_branch()?);
        crate::process::git(&["push", "origin", &refspec])
    }

    /// `pkg_name` is embedded literally in both callbacks so
    /// multi-semantic-release needs no plugin-context env vars at runtime.
    fn releaserc_json(
        &self,
        pixi: &Path,
        pkg_name: &PackageName,
        pass: &Pass,
    ) -> color_eyre::eyre::Result<String> {
        let branches = self
            .release_branches
            .split(',')
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(|b| {
                if b == "alpha" || b.starts_with("alpha/") {
                    serde_json::json!({ "name": b, "prerelease": true })
                } else {
                    serde_json::Value::String(b.to_string())
                }
            })
            .collect::<Vec<_>>();

        let abs_pixi = absolute(pixi);
        let abs_pkgdir = absolute(&self.package_dir);
        let mut prepare_cmd = format!(
            "mise ci verify-siblings --pixi-toml={pixi} --package-dir={pkgdir} && \
             mise ci bump-pixi --version=${{nextRelease.version}} --pixi-toml={pixi}",
            pixi = abs_pixi.display(),
            pkgdir = abs_pkgdir.display(),
        );
        if let Some(extra) = &self.extra_prepare_cmd {
            prepare_cmd.push_str(" && ");
            prepare_cmd.push_str(extra);
        }

        let publish_cmd = format!(
            "mise ci recipes-pr --version=${{nextRelease.version}} --recipes-repo={} --package-dir={} --package={} --sha=${{nextRelease.gitHead}}",
            self.recipes_repo,
            abs_pkgdir.display(),
            pkg_name,
        );

        let mut plugins: Vec<serde_json::Value> = vec![
            serde_json::json!(["@semantic-release/commit-analyzer", { "preset": "conventionalcommits" }]),
            serde_json::json!(["@semantic-release/release-notes-generator", { "preset": "conventionalcommits" }]),
        ];
        match pass {
            Pass::Record { dir } => plugins.push(serde_json::json!([
                dir.join("record_release.js"),
                { "dir": dir, "name": pkg_name }
            ])),
            Pass::Release {
                changelog_committed: false,
            } => plugins.push(serde_json::json!(["@semantic-release/changelog", {}])),
            Pass::Release {
                changelog_committed: true,
            } => {}
        }
        plugins.push(serde_json::json!(["@semantic-release/exec", {
            "prepareCmd": prepare_cmd,
            "publishCmd": publish_cmd,
        }]));

        if self.github_release {
            plugins.push(serde_json::json!([
                "@semantic-release/github",
                { "assets": [], "successComment": false }
            ]));
        }
        // The git plugin is unconditional: versions are read from pixi.toml at
        // the tagged rev, so the bump must always be committed. --changelog
        // only controls the CHANGELOG.md asset.
        let mut assets: Vec<String> = Vec::new();
        if self.changelog {
            assets.push("CHANGELOG.md".to_string());
        }
        assets.push("**/pixi.toml".to_string());
        assets.extend(self.extra_git_asset.iter().cloned());
        // Names the package in the commit subject; otherwise matches
        // @semantic-release/git's default message.
        let git_message = format!(
            "chore(release): {pkg_name} ${{nextRelease.version}} [skip ci]\n\n${{nextRelease.notes}}",
        );
        plugins.push(serde_json::json!([
            "@semantic-release/git",
            { "assets": assets, "message": git_message }
        ]));

        let releaserc = serde_json::json!({
            "branches": branches,
            "plugins": plugins,
        });
        Ok(serde_json::to_string_pretty(&releaserc)?)
    }
}

#[cfg(test)]
#[path = "release_tests.rs"]
mod tests;
