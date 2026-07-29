use clap::Args;
use std::path::PathBuf;

/// mise-specific prepare callback. Run via `mise ci release --extra-prepare-cmd`
/// after bump-pixi and before the @semantic-release/git commit, so Cargo.toml
/// and Cargo.lock land in the same `chore(release)` commit (and tag) as
/// pixi.toml — keeping `mise --version` (CARGO_PKG_VERSION) in step with the
/// released package without a second follow-up commit.
#[derive(Args, Debug)]
pub struct SyncCargo {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: String,
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
    pub fn run(self) -> anyhow::Result<()> {
        let toml = std::fs::read_to_string(&self.cargo_toml)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", self.cargo_toml.display()))?;
        std::fs::write(&self.cargo_toml, bump_cargo_toml(&toml, &self.version)?)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", self.cargo_toml.display()))?;

        let lock = std::fs::read_to_string(&self.cargo_lock)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", self.cargo_lock.display()))?;
        std::fs::write(
            &self.cargo_lock,
            bump_cargo_lock(&lock, &self.package, &self.version)?,
        )
        .map_err(|e| anyhow::anyhow!("writing {}: {e}", self.cargo_lock.display()))?;

        println!("Synced Cargo.toml/Cargo.lock to version {}", self.version);
        Ok(())
    }
}

fn bump_cargo_toml(body: &str, new_version: &str) -> anyhow::Result<String> {
    let mut doc: toml_edit::DocumentMut = body
        .parse()
        .map_err(|e| anyhow::anyhow!("parsing Cargo.toml: {e}"))?;
    let pkg = doc
        .get_mut("package")
        .ok_or_else(|| anyhow::anyhow!("no [package] table in Cargo.toml"))?
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[package] is not a table"))?;
    if !pkg.contains_key("version") {
        anyhow::bail!("no version key in [package] table");
    }
    pkg["version"] = toml_edit::value(new_version);
    Ok(doc.to_string())
}

fn bump_cargo_lock(body: &str, package: &str, new_version: &str) -> anyhow::Result<String> {
    let mut doc: toml_edit::DocumentMut = body
        .parse()
        .map_err(|e| anyhow::anyhow!("parsing Cargo.lock: {e}"))?;
    let pkgs = doc
        .get_mut("package")
        .and_then(|p| p.as_array_of_tables_mut())
        .ok_or_else(|| anyhow::anyhow!("no [[package]] entries in Cargo.lock"))?;
    let entry = pkgs
        .iter_mut()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some(package))
        .ok_or_else(|| anyhow::anyhow!("no [[package]] named {package:?} in Cargo.lock"))?;
    entry["version"] = toml_edit::value(new_version);
    Ok(doc.to_string())
}

#[cfg(test)]
#[path = "sync_cargo_tests.rs"]
mod tests;
