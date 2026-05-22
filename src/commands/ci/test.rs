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
    /// ROS distro identifier (passed to pixi-env tasks).
    #[arg(long, default_value = "kilted")]
    pub ros_distro: String,
}

impl Test {
    pub fn run(self) -> anyhow::Result<()> {
        anyhow::bail!("mise ci test: not yet implemented")
    }
}
