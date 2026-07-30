use anyhow::Context;
use clap::Args;
use std::path::PathBuf;

/// Called by semantic-release's @semantic-release/exec plugin in the prepare
/// phase, before the @semantic-release/git plugin commits. Writes the new
/// version into the package's pixi.toml [package] section.
#[derive(Args, Debug)]
pub struct BumpPixi {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: crate::types::Version,
    /// Path to the package's pixi.toml. Defaults to ./pixi.toml.
    #[arg(long, default_value = crate::consts::PIXI_TOML)]
    pub pixi_toml: PathBuf,
}

impl BumpPixi {
    pub fn run(self) -> anyhow::Result<()> {
        let body = std::fs::read_to_string(&self.pixi_toml)
            .with_context(|| format!("reading {}", self.pixi_toml.display()))?;
        let new_body = crate::manifest::set_package_version(&body, &self.version)
            .with_context(|| format!("bumping {}", self.pixi_toml.display()))?;
        std::fs::write(&self.pixi_toml, new_body)
            .with_context(|| format!("writing {}", self.pixi_toml.display()))?;
        println!(
            "Bumped {} to version {}",
            self.pixi_toml.display(),
            self.version
        );
        Ok(())
    }
}
