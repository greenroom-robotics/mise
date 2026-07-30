use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::consts::{PIXI_NATIVE_PACKAGES_YAML, PIXI_TOML};
use crate::types::{DeepstreamVersion, PixiNativeManifest, RecipeName};

#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Walk up from cwd looking for `pixi.toml`.
    pub fn discover() -> anyhow::Result<Self> {
        let cwd = env::current_dir().context("get current dir")?;
        Self::discover_from(&cwd)
    }

    /// Walk up from `start` looking for `pixi.toml`.
    pub fn discover_from(start: &Path) -> anyhow::Result<Self> {
        let mut cur: &Path = start;
        loop {
            if cur.join(PIXI_TOML).is_file() {
                return Ok(Self {
                    root: cur.canonicalize().context("canonicalize repo root")?,
                });
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => anyhow::bail!("no pixi.toml found walking up from {}", start.display()),
            }
        }
    }

    /// Use an explicit path. Must contain `pixi.toml`.
    pub fn at(root: PathBuf) -> anyhow::Result<Self> {
        if !root.join(PIXI_TOML).is_file() {
            anyhow::bail!("{} does not contain pixi.toml", root.display());
        }
        Ok(Self {
            root: root.canonicalize().context("canonicalize repo root")?,
        })
    }

    /// Use `--repo-root <PATH>` if given, otherwise walk up from cwd looking for `pixi.toml`.
    pub fn or_discover(root: Option<PathBuf>) -> anyhow::Result<Self> {
        match root {
            Some(p) => Self::at(p),
            None => Self::discover(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn deepstream(&self) -> anyhow::Result<DeepstreamCfg> {
        let recipes_path = self.root.join(".github").join("deepstream-recipes.yaml");
        let variants_path = self.root.join("variants").join("deepstream.yaml");

        let recipes_text = fs::read_to_string(&recipes_path)
            .with_context(|| format!("read {}", recipes_path.display()))?;
        let recipes_raw: RecipesRaw = serde_yaml_ng::from_str(&recipes_text)
            .with_context(|| format!("parse {}", recipes_path.display()))?;

        let variants_text = fs::read_to_string(&variants_path)
            .with_context(|| format!("read {}", variants_path.display()))?;
        let variants_raw: VariantsRaw = serde_yaml_ng::from_str(&variants_text)
            .with_context(|| format!("parse {}", variants_path.display()))?;

        Ok(DeepstreamCfg {
            recipes: recipes_raw.recipes.into_iter().collect(),
            versions: variants_raw.deepstream_version.into_iter().collect(),
        })
    }

    pub fn pixi_native_manifest(&self) -> anyhow::Result<PixiNativeManifest> {
        let path = self.root.join(PIXI_NATIVE_PACKAGES_YAML);
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        PixiNativeManifest::from_yaml_str(&text)
            .with_context(|| format!("parse {}", path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct DeepstreamCfg {
    pub recipes: BTreeSet<RecipeName>,
    pub versions: BTreeSet<DeepstreamVersion>,
}

#[derive(Deserialize)]
struct RecipesRaw {
    recipes: Vec<RecipeName>,
}

#[derive(Deserialize)]
struct VariantsRaw {
    deepstream_version: Vec<DeepstreamVersion>,
}

#[cfg(test)]
#[path = "repo_tests.rs"]
mod tests;
