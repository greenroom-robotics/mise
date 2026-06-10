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
    #[arg(long, default_value_t = true)]
    pub changelog: bool,
    /// Comma-separated branch list passed to semantic-release.
    #[arg(long, default_value = "main,master,alpha")]
    pub release_branches: String,
    /// Whether to create a GitHub release.
    #[arg(long, default_value_t = true)]
    pub github_release: bool,
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
        let tag_format = if multi {
            "${name}@${version}"
        } else {
            "${version}"
        };

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
                { "assets": ["CHANGELOG.md", "**/pixi.toml"] }
            ]));
        }

        let releaserc = serde_json::json!({
            "branches": branches,
            "plugins": plugins,
        });
        Ok(serde_json::to_string_pretty(&releaserc)?)
    }
}
