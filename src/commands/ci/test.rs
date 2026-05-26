use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct Test {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
}

impl Test {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs =
            crate::commands::ci::packages::discover(&self.package_dir, self.package.as_deref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        for pixi in pkgs {
            let pkg_dir = pixi.parent().unwrap();
            println!("==> mise ci test :: {}", pkg_dir.display());
            let status = std::process::Command::new("pixi")
                .arg("run")
                .arg("--manifest-path")
                .arg(&pixi)
                .arg("-e")
                .arg("tests")
                .arg("test")
                .status()
                .map_err(|e| anyhow::anyhow!("failed to spawn pixi: {e}"))?;
            if !status.success() {
                anyhow::bail!("pixi run -e tests test failed for {}", pkg_dir.display());
            }
        }
        Ok(())
    }
}
