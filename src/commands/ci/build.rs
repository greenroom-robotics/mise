use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct Build {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// ROS distro identifier.
    #[arg(long, default_value = "kilted")]
    pub ros_distro: String,
    /// rattler-build target subdir.
    #[arg(long)]
    pub target_platform: Option<String>,
}

impl Build {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs =
            crate::commands::ci::packages::discover(&self.package_dir, self.package.as_deref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let out_dir = std::env::var("RUNNER_TEMP")
            .map(|t| std::path::PathBuf::from(t).join("conda-bld"))
            .unwrap_or_else(|_| std::path::PathBuf::from("./output"));
        std::fs::create_dir_all(&out_dir)?;

        for pixi in pkgs {
            let pkg_dir = pixi.parent().unwrap();
            println!("==> mise ci build :: {}", pkg_dir.display());
            let mut cmd = std::process::Command::new("pixi");
            cmd.arg("build")
                .arg("--manifest-path")
                .arg(&pixi)
                .arg("--output-dir")
                .arg(&out_dir);
            if let Some(plat) = &self.target_platform {
                cmd.arg("--target-platform").arg(plat);
            }
            let status = cmd
                .status()
                .map_err(|e| anyhow::anyhow!("failed to spawn pixi: {e}"))?;
            if !status.success() {
                anyhow::bail!("pixi build failed for {}", pkg_dir.display());
            }
        }
        Ok(())
    }
}
