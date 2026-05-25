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
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_pixi(tmp: &Path, body: &str) -> PathBuf {
        let p = tmp.join("pixi.toml");
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn reads_name_and_version() {
        let tmp = TempDir::new().unwrap();
        let p = write_pixi(
            tmp.path(),
            r#"
[workspace]
name = "foo"
[package]
name = "foo"
version = "1.2.3"
"#,
        );
        let pkg = read(&p).unwrap();
        assert_eq!(
            pkg,
            PixiPackage {
                name: "foo".into(),
                version: "1.2.3".into()
            }
        );
    }

    #[test]
    fn errors_when_package_section_missing() {
        let tmp = TempDir::new().unwrap();
        let p = write_pixi(tmp.path(), "[workspace]\nname = \"foo\"\n");
        let err = read(&p).unwrap_err();
        assert!(err.to_string().contains("parsing"));
    }
}
