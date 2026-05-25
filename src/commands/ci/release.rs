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
    #[arg(long, default_value = "greenroom-robotics/ros-kilted-recipes")]
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
        anyhow::bail!("mise ci release: not yet implemented")
    }
}
