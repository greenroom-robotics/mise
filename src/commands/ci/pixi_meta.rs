use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct PixiPackage {
    pub name: String,
    pub version: String,
}

#[derive(Deserialize)]
struct PixiTomlSurface {
    package: PackageSection,
}

#[derive(Deserialize)]
struct PackageSection {
    name: String,
    version: String,
}

pub fn read(pixi_toml: &Path) -> Result<PixiPackage> {
    let body = std::fs::read_to_string(pixi_toml)
        .with_context(|| format!("reading {}", pixi_toml.display()))?;
    let parsed: PixiTomlSurface =
        toml::from_str(&body).with_context(|| format!("parsing {}", pixi_toml.display()))?;
    Ok(PixiPackage {
        name: parsed.package.name,
        version: parsed.package.version,
    })
}

#[cfg(test)]
#[path = "pixi_meta_tests.rs"]
mod tests;
