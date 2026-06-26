use clap::Args;
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

impl Release {
    pub fn run(self) -> anyhow::Result<()> {
        use crate::commands::ci::packages;
        use std::process::Command;

        let pixis = packages::discover(&self.package_dir, self.package.as_deref())?;
        if pixis.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }

        // Write a .releaserc next to each package's pixi.toml. semantic-release
        // (and multi-semantic-release) discover them via the per-package
        // workspaces declared in the root package.json.
        for pixi in &pixis {
            let pkg_dir = pixi.parent().unwrap();
            let releaserc = self.releaserc_json(pixi)?;
            std::fs::write(pkg_dir.join(".releaserc"), releaserc)?;
        }

        // Single-package mode (consumer passed --package, or there's only one
        // package) → bare `semantic-release`. Multi-package mode → `multi-
        // semantic-release` walks workspaces and runs each one independently.
        let multi = self.package.is_none() && pixis.len() > 1;
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

        let prepare_cmd = format!(
            "mise ci bump-pixi --version=${{nextRelease.version}} --pixi-toml={}",
            pixi.display()
        );

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
        if self.changelog {
            plugins.push(serde_json::json!([
                "@semantic-release/git",
                { "assets": ["CHANGELOG.md", "**/package.xml", "**/pixi.toml"] }
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

    #[test]
    fn releaserc_git_plugin_commits_package_xml() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pixi = tmp.path().join("pixi.toml");
        std::fs::write(&pixi, "[package]\nname = \"p\"\nversion = \"1.0.0\"\n").unwrap();

        let release = Release {
            package: Some("p".into()),
            package_dir: tmp.path().to_path_buf(),
            ros_distro: "kilted".into(),
            recipes_repo: "greenroom-robotics/ros-recipes".into(),
            changelog: true,
            release_branches: "main".into(),
            github_release: true,
        };
        let rc = release.releaserc_json(&pixi).unwrap();
        // The git plugin's asset list must include package.xml so the bump persists.
        assert!(rc.contains("**/package.xml"), "releaserc was: {rc}");
    }
}
