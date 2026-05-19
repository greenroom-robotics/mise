use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use crate::types::{DeepstreamVersion, PixiNativeManifest, RecipeName};

#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    /// Walk up from `cwd` looking for `pixi.toml`.
    pub fn discover() -> anyhow::Result<Self> {
        let cwd = env::current_dir().context("get current dir")?;
        let mut cur: &Path = &cwd;
        loop {
            if cur.join("pixi.toml").is_file() {
                return Ok(Self { root: cur.to_path_buf() });
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => anyhow::bail!(
                    "no pixi.toml found walking up from {}",
                    cwd.display()
                ),
            }
        }
    }

    /// Use an explicit path. Must contain `pixi.toml`.
    pub fn at(root: PathBuf) -> anyhow::Result<Self> {
        if !root.join("pixi.toml").is_file() {
            anyhow::bail!("{} does not contain pixi.toml", root.display());
        }
        Ok(Self { root })
    }

    pub fn or_discover(root: Option<PathBuf>) -> anyhow::Result<Self> {
        match root {
            Some(p) => Self::at(p),
            None => Self::discover(),
        }
    }

    pub fn deepstream(&self) -> anyhow::Result<DeepstreamCfg> {
        let path = self.root.join(".github").join("deepstream-recipes.yaml");
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let raw: DeepstreamRaw = serde_yaml::from_str(&text)
            .with_context(|| format!("parse {}", path.display()))?;
        Ok(DeepstreamCfg {
            recipes: raw.recipes.into_iter().collect(),
            versions: raw.deepstream_versions.into_iter().collect(),
        })
    }

    pub fn pixi_native_manifest(&self) -> anyhow::Result<PixiNativeManifest> {
        let path = self.root.join("pixi_native_packages.yaml");
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        PixiNativeManifest::from_yaml_str(&text)
            .with_context(|| format!("parse {}", path.display()))
    }
}

#[derive(Debug, Clone)]
pub struct DeepstreamCfg {
    pub recipes: HashSet<RecipeName>,
    pub versions: HashSet<DeepstreamVersion>,
}

#[derive(Deserialize)]
struct DeepstreamRaw {
    recipes: Vec<RecipeName>,
    deepstream_versions: Vec<DeepstreamVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_repo() -> (TempDir, Repo) {
        let td = TempDir::new().unwrap();
        fs::write(td.path().join("pixi.toml"), "[project]\n").unwrap();
        let repo = Repo::at(td.path().to_path_buf()).unwrap();
        (td, repo)
    }

    #[test]
    fn discover_walks_up() {
        let (td, _) = make_repo();
        let sub = td.path().join("a").join("b");
        fs::create_dir_all(&sub).unwrap();
        env::set_current_dir(&sub).unwrap();
        let found = Repo::discover().unwrap();
        assert_eq!(found.root, td.path().canonicalize().unwrap());
    }

    #[test]
    fn at_rejects_dir_without_pixi_toml() {
        let td = TempDir::new().unwrap();
        assert!(Repo::at(td.path().to_path_buf()).is_err());
    }

    #[test]
    fn deepstream_loader_parses_fixture() {
        let (td, repo) = make_repo();
        let gh = td.path().join(".github");
        fs::create_dir_all(&gh).unwrap();
        fs::copy(
            "tests/fixtures/deepstream-recipes.yaml",
            gh.join("deepstream-recipes.yaml"),
        ).unwrap();
        let cfg = repo.deepstream().unwrap();
        assert_eq!(cfg.recipes.len(), 2);
        assert_eq!(cfg.versions.len(), 2);
        assert!(cfg.versions.contains(&DeepstreamVersion::V7_1));
    }

    #[test]
    fn pixi_native_loader_parses_valid_yaml() {
        let (td, repo) = make_repo();
        fs::write(
            td.path().join("pixi_native_packages.yaml"),
            "packages:\n  - name: foo\n    url: https://github.com/x/y.git\n    rev: 4110a9a40736b555c7419119ef6c607951563745\n",
        ).unwrap();
        let m = repo.pixi_native_manifest().unwrap();
        assert_eq!(m.packages.len(), 1);
    }
}
