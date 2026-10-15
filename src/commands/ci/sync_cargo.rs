use clap::Args;
use color_eyre::eyre::WrapErr;
use std::path::{Path, PathBuf};

use crate::types::Version;

/// Syncs Cargo.toml and Cargo.lock to the released version with explicit paths.
#[derive(Args, Debug)]
pub struct SyncCargo {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: crate::types::Version,
    /// Path to Cargo.toml.
    #[arg(long, default_value = "Cargo.toml")]
    pub cargo_toml: PathBuf,
    /// Path to Cargo.lock.
    #[arg(long, default_value = "Cargo.lock")]
    pub cargo_lock: PathBuf,
    /// Package name whose entry is bumped in Cargo.lock.
    #[arg(long, default_value = "mise")]
    pub package: String,
}

impl SyncCargo {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let toml = std::fs::read_to_string(&self.cargo_toml)
            .with_context(|| format!("reading {}", self.cargo_toml.display()))?;
        let bumped = crate::manifest::set_package_version(&toml, &self.version)
            .with_context(|| format!("bumping {}", self.cargo_toml.display()))?;
        std::fs::write(&self.cargo_toml, bumped)
            .with_context(|| format!("writing {}", self.cargo_toml.display()))?;

        let lock = std::fs::read_to_string(&self.cargo_lock)
            .with_context(|| format!("reading {}", self.cargo_lock.display()))?;
        std::fs::write(
            &self.cargo_lock,
            bump_cargo_lock(&lock, &self.package, &self.version)?,
        )
        .with_context(|| format!("writing {}", self.cargo_lock.display()))?;

        println!("Synced Cargo.toml/Cargo.lock to version {}", self.version);
        Ok(())
    }
}

/// A crate's Cargo.toml and the Cargo.lock nearest above it, searching no
/// further than the enclosing git repository root.
struct CargoFiles {
    toml: PathBuf,
    lock: Option<PathBuf>,
}

impl CargoFiles {
    fn find(dir: &Path) -> color_eyre::eyre::Result<Option<Self>> {
        let toml = dir.join("Cargo.toml");
        let Ok(body) = std::fs::read_to_string(&toml) else {
            return Ok(None);
        };
        if body
            .parse::<toml_edit::DocumentMut>()
            .is_ok_and(|d| !d.contains_key("package"))
        {
            color_eyre::eyre::bail!("{} has no [package] table", toml.display());
        }
        let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_owned());
        let mut lock = None;
        for d in abs.ancestors() {
            lock = Some(d.join("Cargo.lock")).filter(|l| l.is_file());
            if lock.is_some() || d.join(".git").exists() {
                break;
            }
        }
        Ok(Some(Self { toml, lock }))
    }
}

/// Cargo.toml and Cargo.lock that [`sync_package`] would touch for `dir`.
pub fn cargo_files(dir: &Path) -> color_eyre::eyre::Result<Vec<PathBuf>> {
    Ok(CargoFiles::find(dir)?
        .into_iter()
        .flat_map(|f| std::iter::once(f.toml).chain(f.lock))
        .collect())
}

/// Bumps the crate in `dir` and its Cargo.lock entry to `version`.
///
/// A directory without Cargo.toml is left alone; one whose Cargo.toml has
/// no `[package]` is an error. Everything is validated
/// before anything is written; returns the files written.
pub fn sync_package(dir: &Path, version: &Version) -> color_eyre::eyre::Result<Vec<PathBuf>> {
    let Some(files) = CargoFiles::find(dir)? else {
        return Ok(Vec::new());
    };
    let toml = std::fs::read_to_string(&files.toml)
        .with_context(|| format!("reading {}", files.toml.display()))?;
    let name = crate_name(&toml).with_context(|| files.toml.display().to_string())?;
    let new_toml = crate::manifest::set_package_version(&toml, version)
        .with_context(|| format!("bumping {}", files.toml.display()))?;
    let mut writes = vec![(files.toml, new_toml)];
    if let Some(lock_path) = files.lock {
        let lock = std::fs::read_to_string(&lock_path)
            .with_context(|| format!("reading {}", lock_path.display()))?;
        let new_lock = bump_cargo_lock(&lock, &name, version)
            .with_context(|| format!("bumping {}", lock_path.display()))?;
        writes.push((lock_path, new_lock));
    }
    for (path, body) in &writes {
        std::fs::write(path, body).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(writes.into_iter().map(|(path, _)| path).collect())
}

/// `[package].name`, refusing a version inherited from a workspace since
/// bumping it would move every crate that shares it.
fn crate_name(cargo_toml: &str) -> color_eyre::eyre::Result<String> {
    let doc: toml_edit::DocumentMut = cargo_toml.parse().context("parsing Cargo.toml")?;
    let package = doc
        .get("package")
        .ok_or_else(|| color_eyre::eyre::eyre!("no [package] table"))?;
    if package.get("version").is_some_and(|v| !v.is_str()) {
        color_eyre::eyre::bail!("[package].version is not a plain string (workspace-inherited?)");
    }
    package
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::to_owned)
        .ok_or_else(|| color_eyre::eyre::eyre!("no [package].name"))
}

fn bump_cargo_lock(
    body: &str,
    package: &str,
    new_version: &Version,
) -> color_eyre::eyre::Result<String> {
    let mut doc: toml_edit::DocumentMut = body.parse().context("parsing Cargo.lock")?;
    let pkgs = doc
        .get_mut("package")
        .and_then(|p| p.as_array_of_tables_mut())
        .ok_or_else(|| color_eyre::eyre::eyre!("no [[package]] entries in Cargo.lock"))?;
    let entry = pkgs
        .iter_mut()
        .find(|e| {
            e.get("name").and_then(|n| n.as_str()) == Some(package) && !e.contains_key("source")
        })
        .ok_or_else(|| {
            color_eyre::eyre::eyre!("no local [[package]] named {package:?} in Cargo.lock")
        })?;
    entry["version"] = toml_edit::value(new_version.to_string());
    Ok(doc.to_string())
}

#[cfg(test)]
#[path = "sync_cargo_tests.rs"]
mod tests;
