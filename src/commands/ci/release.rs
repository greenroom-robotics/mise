use clap::Args;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct Release {
    /// Single package to release (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// ROS distro identifier.
    #[arg(long, default_value = "kilted")]
    pub ros_distro: String,
    /// owner/repo of the conda recipes repository to upsert into.
    #[arg(long, default_value = "greenroom-robotics/ros-recipes")]
    pub recipes_repo: String,
    /// Whether to commit CHANGELOG.md back to the source repo.
    // ArgAction::Set so the flag takes an explicit value (`--changelog false`);
    // a bare bool flag would reject the `--changelog true` the release action
    // passes.
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
    /// Used by Rust-native repos (e.g. mise itself) to fold Cargo.toml/Cargo.lock
    /// into the release commit; the generic path stays Cargo-agnostic.
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
fn tag_format(multi: bool, single_pkg_name: &str) -> String {
    if multi {
        "${name}@${version}".to_string()
    } else {
        format!("{single_pkg_name}@${{version}}")
    }
}

/// Per-workspace package.json synthesized at release time so the patched
/// multi-semantic-release discovers packages and releases them in topological
/// order of sibling deps. Never committed (repos don't track package.json).
fn package_json_for(name: &str, version: &str, deps: &BTreeSet<String>) -> String {
    let deps_obj: serde_json::Map<String, serde_json::Value> = deps
        .iter()
        .map(|d| (d.clone(), serde_json::Value::String("*".into())))
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "name": name,
        "version": version,
        "private": true,
        "dependencies": deps_obj,
    }))
    .expect("static json")
}

/// Merge a `workspaces` array into the root package.json (staged by the
/// release action), creating a minimal one for standalone/local runs.
fn ensure_root_workspaces(root_pkg_json: &std::path::Path, globs: &[String]) -> anyhow::Result<()> {
    let mut v: serde_json::Value = if root_pkg_json.exists() {
        serde_json::from_str(&std::fs::read_to_string(root_pkg_json)?)?
    } else {
        serde_json::json!({ "name": "mise-release-root", "private": true })
    };
    v["workspaces"] = serde_json::json!(globs);
    std::fs::write(root_pkg_json, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

impl Release {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::commands::ci::packages;
        use std::process::Command;

        let pixis = packages::discover(&self.package_dir, self.package.as_deref())?;
        if pixis.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let multi = self.package.is_none() && pixis.len() > 1;

        // Sibling graph: drives msr's topological release order (multi mode)
        // via synthesized package.json files. Both dep kinds order; only path
        // deps are guarded (verify-siblings).
        let graph = crate::commands::ci::siblings::analyze(&pixis)?;

        // Write a .releaserc (and, in multi mode, a package.json encoding
        // sibling deps) next to each package's pixi.toml.
        let mut workspace_globs: Vec<String> = Vec::new();
        for pixi in &pixis {
            let pkg_dir = pixi.parent().unwrap();
            let releaserc = self.releaserc_json(pixi)?;
            std::fs::write(pkg_dir.join(".releaserc"), releaserc)?;
            if multi {
                let pkg = crate::commands::ci::pixi_meta::read(pixi)?;
                let empty = BTreeSet::new();
                let deps: BTreeSet<String> = graph
                    .path_deps
                    .get(&pkg.name)
                    .unwrap_or(&empty)
                    .union(graph.pin_deps.get(&pkg.name).unwrap_or(&empty))
                    .cloned()
                    .collect();
                std::fs::write(
                    pkg_dir.join("package.json"),
                    package_json_for(&pkg.name, &pkg.version, &deps),
                )?;
                workspace_globs.push(pkg_dir.to_string_lossy().into_owned());
            }
        }
        if multi {
            ensure_root_workspaces(std::path::Path::new("package.json"), &workspace_globs)?;
        }

        // In single-package mode `pixis` holds exactly the package being
        // released, so its name can be embedded literally in the tag format.
        let single_pkg = crate::commands::ci::pixi_meta::read(&pixis[0])?;
        let tag_format = tag_format(multi, &single_pkg.name);

        let bin = if multi {
            "multi-semantic-release"
        } else {
            "semantic-release"
        };
        let mut cmd = Command::new("npx");
        cmd.arg("--no-install")
            .arg(bin)
            .arg(format!("--tag-format={tag_format}"));
        let status = cmd
            .status()
            .map_err(|e| anyhow::anyhow!("failed to spawn npx {bin}: {e}"))?;
        if !status.success() {
            anyhow::bail!("npx {bin} failed");
        }
        Ok(())
    }

    fn releaserc_json(&self, pixi: &std::path::Path) -> anyhow::Result<String> {
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

        // We resolve the package name once now and embed it literally in
        // both callbacks so multi-semantic-release doesn't need any plugin-
        // context env vars at runtime.
        let pkg = crate::commands::ci::pixi_meta::read(pixi)?;

        let mut prepare_cmd = format!(
            "mise ci verify-siblings --pixi-toml={pixi} --package-dir={pkgdir} && \
             mise ci bump-pixi --version=${{nextRelease.version}} --pixi-toml={pixi}",
            pixi = pixi.display(),
            pkgdir = self.package_dir.display(),
        );
        if let Some(extra) = &self.extra_prepare_cmd {
            prepare_cmd.push_str(" && ");
            prepare_cmd.push_str(extra);
        }

        let publish_cmd = format!(
            "mise ci recipes-pr --version=${{nextRelease.version}} --recipes-repo={} --package-dir={} --ros-distro={} --package={} --sha=${{nextRelease.gitHead}}",
            self.recipes_repo,
            self.package_dir.display(),
            self.ros_distro,
            pkg.name,
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
        if self.changelog || !self.extra_git_asset.is_empty() {
            let mut assets: Vec<String> = Vec::new();
            if self.changelog {
                assets.push("CHANGELOG.md".to_string());
            }
            assets.push("**/pixi.toml".to_string());
            assets.extend(self.extra_git_asset.iter().cloned());
            plugins.push(serde_json::json!([
                "@semantic-release/git",
                { "assets": assets }
            ]));
        }

        let releaserc = serde_json::json!({
            "branches": branches,
            "plugins": plugins,
        });
        Ok(serde_json::to_string_pretty(&releaserc)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // Minimal parser wrapper so we can exercise Release's arg definitions.
    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(flatten)]
        release: Release,
    }

    #[test]
    fn changelog_accepts_explicit_bool_value() {
        // Matches what the release composite action passes.
        let cli = TestCli::parse_from(["x", "--package-dir", ".", "--changelog", "true"]);
        assert!(cli.release.changelog);

        let cli = TestCli::parse_from(["x", "--changelog", "false"]);
        assert!(!cli.release.changelog);
    }

    #[test]
    fn bool_flags_default_to_true_when_omitted() {
        let cli = TestCli::parse_from(["x"]);
        assert!(cli.release.changelog);
        assert!(cli.release.github_release);
    }

    #[test]
    fn github_release_accepts_explicit_bool_value() {
        let cli = TestCli::parse_from(["x", "--github-release", "false"]);
        assert!(!cli.release.github_release);
    }

    // Single-package mode must keep the same `<name>@<version>` tag convention
    // as multi-package mode; a bare `${version}` format makes semantic-release
    // ignore all existing `<name>@X.Y.Z` tags and restart at 1.0.0.
    #[test]
    fn single_package_tag_format_embeds_package_name() {
        assert_eq!(tag_format(false, "mise"), "mise@${version}");
    }

    #[test]
    fn multi_package_tag_format_uses_msr_name_placeholder() {
        assert_eq!(tag_format(true, "mise"), "${name}@${version}");
    }

    // Extra prepare cmd + git assets must reach the .releaserc so the Cargo bump
    // lands in the same release commit/tag as pixi.toml.
    #[test]
    fn extra_prepare_cmd_and_git_assets_appear_in_releaserc() {
        let dir = std::env::temp_dir().join("mise-release-test");
        std::fs::create_dir_all(&dir).unwrap();
        let pixi = dir.join("pixi.toml");
        std::fs::write(&pixi, "[package]\nname = \"mise\"\nversion = \"1.0.0\"\n").unwrap();

        let cli = TestCli::parse_from([
            "x",
            "--extra-prepare-cmd",
            "mise ci sync-cargo --version=${nextRelease.version}",
            "--extra-git-asset",
            "Cargo.toml",
            "--extra-git-asset",
            "Cargo.lock",
        ]);
        let rc = cli.release.releaserc_json(&pixi).unwrap();
        let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
        let plugins = v["plugins"].as_array().unwrap();

        let exec = plugins
            .iter()
            .find(|p| p[0] == "@semantic-release/exec")
            .unwrap();
        let prepare = exec[1]["prepareCmd"].as_str().unwrap();
        assert!(prepare.contains("mise ci bump-pixi"));
        assert!(prepare.contains(" && mise ci sync-cargo --version=${nextRelease.version}"));

        let git = plugins
            .iter()
            .find(|p| p[0] == "@semantic-release/git")
            .unwrap();
        let assets = git[1]["assets"].as_array().unwrap();
        assert!(assets.iter().any(|a| a == "**/pixi.toml"));
        assert!(assets.iter().any(|a| a == "Cargo.toml"));
        assert!(assets.iter().any(|a| a == "Cargo.lock"));
    }

    #[test]
    fn prepare_cmd_runs_verify_siblings_before_bump() {
        let dir = std::env::temp_dir().join("mise-release-verify-test");
        std::fs::create_dir_all(&dir).unwrap();
        let pixi = dir.join("pixi.toml");
        std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
        let cli = TestCli::parse_from(["x"]);
        let rc = cli.release.releaserc_json(&pixi).unwrap();
        let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
        let prepare = v["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p[0] == "@semantic-release/exec")
            .unwrap()[1]["prepareCmd"]
            .as_str()
            .unwrap()
            .to_string();
        let verify_pos = prepare
            .find("verify-siblings")
            .expect("verify-siblings present");
        let bump_pos = prepare.find("bump-pixi").unwrap();
        assert!(
            verify_pos < bump_pos,
            "guard must run before the bump: {prepare}"
        );
    }

    #[test]
    fn package_json_encodes_sibling_deps_for_msr_ordering() {
        let mut deps = std::collections::BTreeSet::new();
        deps.insert("geolocation".to_string());
        let js = package_json_for("geolocation_node", "1.37.0", &deps);
        let v: serde_json::Value = serde_json::from_str(&js).unwrap();
        assert_eq!(v["name"], "geolocation_node");
        assert_eq!(v["version"], "1.37.0");
        assert_eq!(v["private"], true);
        assert_eq!(v["dependencies"]["geolocation"], "*");
    }

    #[test]
    fn ensure_root_workspaces_merges_into_existing_tooling_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("package.json");
        std::fs::write(&root, r#"{"name":"mise-release-tooling","private":true}"#).unwrap();
        ensure_root_workspaces(&root, &["packages/geolocation".into()]).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&root).unwrap()).unwrap();
        assert_eq!(v["name"], "mise-release-tooling"); // existing fields kept
        assert_eq!(v["workspaces"][0], "packages/geolocation");
    }

    #[test]
    fn ensure_root_workspaces_creates_minimal_json_when_absent() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("package.json");
        ensure_root_workspaces(&root, &["packages/a".into(), "packages/b".into()]).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&root).unwrap()).unwrap();
        assert_eq!(v["private"], true);
        assert_eq!(v["workspaces"].as_array().unwrap().len(), 2);
    }
}
