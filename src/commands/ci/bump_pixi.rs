use clap::Args;
use color_eyre::eyre::WrapErr;
use std::path::PathBuf;

/// Writes the new version into each package's pixi.toml [package] section.
/// All manifests are read and validated before any is written, so a bad
/// path leaves every file untouched.
#[derive(Args, Debug)]
pub struct BumpPixi {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: crate::types::Version,
    /// Path(s) to the pixi.toml of each package to bump. Defaults to ./pixi.toml.
    #[arg(long, num_args = 1.., default_value = crate::consts::PIXI_TOML)]
    pub pixi_toml: Vec<PathBuf>,
}

impl BumpPixi {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let bumped = self
            .pixi_toml
            .iter()
            .map(|path| {
                let body = std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let new_body = crate::manifest::set_package_version(&body, &self.version)
                    .with_context(|| format!("bumping {}", path.display()))?;
                Ok((path, new_body))
            })
            .collect::<color_eyre::eyre::Result<Vec<_>>>()?;
        for (path, new_body) in bumped {
            std::fs::write(path, new_body)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("Bumped {} to version {}", path.display(), self.version);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bump_pixi_tests.rs"]
mod tests;
