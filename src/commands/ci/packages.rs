use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// True when the manifest declares a `[package]` table.
///
/// A workspace-only manifest is a dev/test environment for something this repo
/// does not publish — e.g. a DeepStream-linked package whose artifact comes
/// from a hand-authored recipe in ros-recipes, but which still wants a pixi env
/// for colcon and the ROS dev tools. It has no version to release and nothing
/// for `pixi build` to build, so discovery skips it. Genuine TOML syntax errors
/// still propagate: only a *valid* manifest with no `[package]` is skippable.
pub(crate) fn declares_package(pixi_toml: &Path) -> Result<bool> {
    let text = std::fs::read_to_string(pixi_toml)
        .with_context(|| format!("reading {}", pixi_toml.display()))?;
    let doc: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", pixi_toml.display()))?;
    Ok(doc.get("package").is_some())
}

/// Discover per-package pixi workspaces under `package_dir`.
///
/// If `filter` is `Some(name)`, returns only that package (errors if missing).
/// Manifests with no `[package]` table are skipped — see `declares_package`.
/// Returns absolute paths to each package's `pixi.toml`.
pub fn discover(package_dir: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>> {
    // Root-package layout: package_dir itself holds the package's pixi.toml
    // (e.g. mise — a single-package repo with pixi.toml at its root). Only take
    // this branch when the root pixi.toml actually declares a [package]; a
    // workspace-only root manifest falls through to the per-subdir scan so it
    // doesn't shadow real packages under package_dir.
    let root_pixi = package_dir.join("pixi.toml");
    if root_pixi.exists()
        && let Ok(pkg) = crate::commands::ci::pixi_meta::read(&root_pixi)
    {
        match filter {
            Some(name) if name != pkg.name => {
                anyhow::bail!("package {name} not found; root package is {}", pkg.name)
            }
            _ => return Ok(vec![root_pixi]),
        }
    }

    if let Some(name) = filter {
        let pixi = package_dir.join(name).join("pixi.toml");
        if !pixi.exists() {
            anyhow::bail!("package {name} not found at {}", pixi.display());
        }
        // An explicit request names something unreleasable — say so, rather
        // than letting the caller trip over `missing field package` later.
        if !declares_package(&pixi)? {
            anyhow::bail!(
                "package {name} has no [package] section in {} — workspace-only \
                 manifests are dev environments, not releasable packages",
                pixi.display()
            );
        }
        return Ok(vec![pixi]);
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(package_dir)
        .with_context(|| format!("reading {}", package_dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let pixi = entry.path().join("pixi.toml");
        if pixi.exists() && declares_package(&pixi)? {
            out.push(pixi);
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
#[path = "packages_tests.rs"]
mod tests;
