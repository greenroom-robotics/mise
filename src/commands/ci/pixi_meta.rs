use anyhow::{Context, Result};
use regex::Regex;
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
    // ROS package-xml mode: a package.xml beside the pixi.toml is the source of
    // truth for name + version. This mirrors the backend's mode resolution.
    if let Some(dir) = pixi_toml.parent() {
        let package_xml = dir.join("package.xml");
        if package_xml.exists() {
            return read_package_xml(&package_xml);
        }
    }

    // Non-ROS repos (e.g. mise itself) carry name/version in pixi.toml [package].
    let body = std::fs::read_to_string(pixi_toml)
        .with_context(|| format!("reading {}", pixi_toml.display()))?;
    let parsed: PixiTomlSurface =
        toml::from_str(&body).with_context(|| format!("parsing {}", pixi_toml.display()))?;
    Ok(PixiPackage {
        name: parsed.package.name,
        version: parsed.package.version,
    })
}

/// Extract `<name>` and `<version>` element text from a package.xml. The
/// regex matches the *elements* only — `<depend version_gte="...">` and similar
/// attributes are never matched because they are not `<version>...</version>`.
fn read_package_xml(path: &Path) -> Result<PixiPackage> {
    let body =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = first_element(&body, "name")
        .with_context(|| format!("no <name> element in {}", path.display()))?;
    let version = first_element(&body, "version")
        .with_context(|| format!("no <version> element in {}", path.display()))?;
    Ok(PixiPackage { name, version })
}

fn first_element(body: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?s)<{tag}>\s*(.*?)\s*</{tag}>")).ok()?;
    re.captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
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

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
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

    #[test]
    fn prefers_package_xml_when_present() {
        let tmp = TempDir::new().unwrap();
        let pixi = write_file(
            tmp.path(),
            "pixi.toml",
            // Intentionally a different (stale) name/version to prove package.xml wins.
            "[package]\nname = \"stale\"\nversion = \"0.0.0\"\n",
        );
        write_file(
            tmp.path(),
            "package.xml",
            r#"<?xml version="1.0"?>
<package format="3">
  <name>release_testing_msgs</name>
  <version>2.0.1</version>
  <depend version_gte="1.0.0">std_msgs</depend>
</package>
"#,
        );
        let pkg = read(&pixi).unwrap();
        // Unprefixed name straight from package.xml; the <depend version_gte> attribute
        // must NOT be mistaken for the package <version>.
        assert_eq!(
            pkg,
            PixiPackage {
                name: "release_testing_msgs".into(),
                version: "2.0.1".into()
            }
        );
    }

    #[test]
    fn falls_back_to_pixi_toml_without_package_xml() {
        let tmp = TempDir::new().unwrap();
        let pixi = write_file(
            tmp.path(),
            "pixi.toml",
            "[package]\nname = \"mise\"\nversion = \"4.5.2\"\n",
        );
        let pkg = read(&pixi).unwrap();
        assert_eq!(
            pkg,
            PixiPackage {
                name: "mise".into(),
                version: "4.5.2".into()
            }
        );
    }
}
