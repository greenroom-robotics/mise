use clap::Args;
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::types::{PackageName, Version};

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
fn package_json_for(name: &PackageName, version: &Version, deps: &BTreeSet<PackageName>) -> String {
    let deps_obj: serde_json::Map<String, serde_json::Value> = deps
        .iter()
        .map(|d| (d.to_string(), serde_json::Value::String("*".into())))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": name,
        "version": version.to_string(),
        "dependencies": deps_obj,
    }))
    .expect("static json")
}

/// Absolutize a path against cwd. multi-semantic-release runs each package's
/// semantic-release — and therefore the exec plugin's shell commands — with
/// cwd = the package directory, so paths embedded in prepareCmd/publishCmd
/// must be absolute to survive.
fn absolute(p: &std::path::Path) -> std::path::PathBuf {
    if p.is_absolute() {
        p.to_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_owned())
    }
}

/// Convert an absolute path to a relative path from cwd if possible;
/// otherwise return the path unchanged. Workspace globs in package.json
/// must be cwd-relative for npm/yarn/msr discovery to work correctly.
fn cwd_relative(p: &std::path::Path) -> std::path::PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| p.strip_prefix(&cwd).ok())
        .map(|r| r.to_owned())
        .unwrap_or_else(|| p.to_owned())
}

/// Merge a `workspaces` array into the root package.json, creating a minimal
/// one if absent.
fn ensure_root_workspaces(root_pkg_json: &std::path::Path, globs: &[String]) -> anyhow::Result<()> {
    use anyhow::Context;
    let mut v: serde_json::Value = if root_pkg_json.exists() {
        let text = std::fs::read_to_string(root_pkg_json)
            .with_context(|| format!("reading {}", root_pkg_json.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", root_pkg_json.display()))?
    } else {
        serde_json::json!({ "name": "mise-release-root", "private": true })
    };
    v["workspaces"] = serde_json::json!(globs);
    std::fs::write(root_pkg_json, serde_json::to_string_pretty(&v)?)
        .with_context(|| format!("writing {}", root_pkg_json.display()))?;
    Ok(())
}

impl Release {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let multi = self.package.is_none() && pkgs.len() > 1;

        let graph = crate::commands::ci::siblings::analyze(&pkgs);

        let mut workspace_globs: Vec<String> = Vec::new();
        for pkg in &pkgs {
            let pkg_dir = &pkg.dir;
            let id = pkg.identity();
            let releaserc = self.releaserc_json(&pkg.manifest_path, &id.name)?;
            std::fs::write(pkg_dir.join(".releaserc"), releaserc)?;
            if multi {
                let deps = msr_ordering_deps(&graph, &id.name);
                std::fs::write(
                    pkg_dir.join("package.json"),
                    package_json_for(&id.name, &id.version, &deps),
                )?;
                let rel_pkg_dir = cwd_relative(pkg_dir);
                workspace_globs.push(rel_pkg_dir.to_string_lossy().into_owned());
            }
        }
        if multi {
            ensure_root_workspaces(std::path::Path::new("package.json"), &workspace_globs)?;
        }
        // Plain semantic-release resolves its config from cwd — a .releaserc
        // down in the package dir is invisible to it, and its own defaults
        // have no `main` branch (ERELEASEBRANCHES).
        if !multi {
            let pkg = &pkgs[0];
            let releaserc = self.releaserc_json(&pkg.manifest_path, &pkg.identity().name)?;
            std::fs::write(".releaserc", releaserc)?;
        }

        // Multi mode ignores the name; single mode's pkgs[0] is the one package.
        let tag_format = tag_format(multi, &pkgs[0].identity().name);

        let argv = release_argv(multi, &tag_format);
        crate::process::run("npx", &argv)
    }

    /// `pkg_name` is embedded literally in both callbacks so
    /// multi-semantic-release needs no plugin-context env vars at runtime.
    fn releaserc_json(
        &self,
        pixi: &std::path::Path,
        pkg_name: &PackageName,
    ) -> anyhow::Result<String> {
        let branches = self
            .release_branches
            .split(',')
            .map(|b| b.trim())
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
            serde_json::json!(["@semantic-release/changelog", {}]),
            serde_json::json!(["@semantic-release/exec", {
                "prepareCmd": prepare_cmd,
                "publishCmd": publish_cmd,
            }]),
        ];

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
            "chore(release): {name} ${{nextRelease.version}} [skip ci]\n\n${{nextRelease.notes}}",
            name = pkg_name,
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
