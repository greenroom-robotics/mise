use clap::Args;
use std::path::PathBuf;

/// Called by semantic-release's @semantic-release/exec plugin in the prepare
/// phase, before the @semantic-release/git plugin commits. Writes the new
/// version into the package's pixi.toml [package] section.
#[derive(Args, Debug)]
pub struct BumpPixi {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: String,
    /// Path to the package's pixi.toml. Defaults to ./pixi.toml.
    #[arg(long, default_value = "pixi.toml")]
    pub pixi_toml: PathBuf,
}

impl BumpPixi {
    pub fn run(self) -> anyhow::Result<()> {
        anyhow::bail!("mise ci bump-pixi: not yet implemented")
    }
}
