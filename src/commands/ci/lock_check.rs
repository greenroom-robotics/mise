use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct LockCheck {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
}

impl LockCheck {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs =
            crate::commands::ci::packages::discover(&self.package_dir, self.package.as_deref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let mut failed = Vec::new();
        for pixi in pkgs {
            let pkg_dir = pixi.parent().unwrap();
            println!("==> mise ci lock-check :: {}", pkg_dir.display());
            // `pixi lock --check` verifies pixi.lock is present and in sync with
            // pixi.toml without installing; it exits non-zero on drift/missing.
            let status = std::process::Command::new("pixi")
                .arg("lock")
                .arg("--check")
                .arg("--manifest-path")
                .arg(&pixi)
                .status()
                .map_err(|e| anyhow::anyhow!("failed to spawn pixi: {e}"))?;
            if !status.success() {
                failed.push(pkg_dir.display().to_string());
            }
        }
        if !failed.is_empty() {
            anyhow::bail!("lockfile check failed for: {}", failed.join(", "));
        }
        Ok(())
    }
}
