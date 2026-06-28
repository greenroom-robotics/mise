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
mod tests {
    use super::*;

    #[test]
    fn bumps_only_the_package_version_in_cargo_toml() {
        let before = r#"[package]
name = "mise"
version = "1.0.0"
edition = "2021"

[dependencies]
anyhow = "1.0.0"
"#;
        let after = bump_cargo_toml(before, "2.3.4").unwrap();
        let doc: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(doc["package"]["version"].as_str(), Some("2.3.4"));
        // The lookalike dependency version line must not move.
        assert_eq!(doc["dependencies"]["anyhow"].as_str(), Some("1.0.0"));
    }

    #[test]
    fn bumps_named_package_in_cargo_lock_only() {
        let before = r#"version = 3

[[package]]
name = "anyhow"
version = "1.0.0"

[[package]]
name = "mise"
version = "1.0.0"
dependencies = ["anyhow"]
"#;
        let after = bump_cargo_lock(before, "mise", "2.3.4").unwrap();
        let doc: toml_edit::DocumentMut = after.parse().unwrap();
        let pkgs = doc["package"].as_array_of_tables().unwrap();
        for p in pkgs {
            let want = if p["name"].as_str() == Some("mise") {
                "2.3.4"
            } else {
                "1.0.0"
            };
            assert_eq!(p["version"].as_str(), Some(want));
        }
    }

    #[test]
    fn errors_when_package_absent_from_lock() {
        let before = "version = 3\n\n[[package]]\nname = \"anyhow\"\nversion = \"1.0.0\"\n";
        let err = bump_cargo_lock(before, "mise", "2.3.4").unwrap_err();
        assert!(err.to_string().contains("mise"));
    }
}
