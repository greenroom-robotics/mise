use clap::Args;
use std::path::PathBuf;

/// Called by semantic-release's @semantic-release/exec plugin in the prepare
/// phase, before the @semantic-release/git plugin commits. Writes the new
/// version into the package's pixi.toml [package] section.
#[derive(Args, Debug)]
pub struct BumpPixi {
    /// New version, no leading 'v' (matches `${nextRelease.version}`).
    #[arg(long)]
    pub version: String,
    /// Path to the package's pixi.toml. Defaults to ./pixi.toml.
    #[arg(long, default_value = "pixi.toml")]
    pub pixi_toml: PathBuf,
}

impl BumpPixi {
    pub fn run(self) -> anyhow::Result<()> {
        let body = std::fs::read_to_string(&self.pixi_toml)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", self.pixi_toml.display()))?;
        let new_body = bump_toml(&body, &self.version)?;
        std::fs::write(&self.pixi_toml, new_body)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", self.pixi_toml.display()))?;
        println!(
            "Bumped {} to version {}",
            self.pixi_toml.display(),
            self.version
        );
        Ok(())
    }
}

fn bump_toml(body: &str, new_version: &str) -> anyhow::Result<String> {
    let mut doc: toml_edit::DocumentMut = body
        .parse()
        .map_err(|e| anyhow::anyhow!("parsing pixi.toml: {e}"))?;

    let pkg = doc
        .get_mut("package")
        .ok_or_else(|| anyhow::anyhow!("no [package] table found"))?
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[package] is not a table"))?;

    if !pkg.contains_key("version") {
        anyhow::bail!("no version key in [package] table");
    }
    pkg["version"] = toml_edit::value(new_version);

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps_version_in_package_table() {
        let before = r#"[workspace]
name = "foo"
[package]
name = "foo"
version = "1.0.0"
description = "Test"
"#;
        let after = bump_toml(before, "1.2.3").unwrap();
        assert!(after.contains(r#"version = "1.2.3""#));
        assert!(!after.contains(r#"version = "1.0.0""#));
        assert!(after.contains(r#"description = "Test""#));
        assert!(after.contains(r#"name = "foo""#));
    }

    #[test]
    fn does_not_touch_workspace_version() {
        let before = r#"[workspace]
name = "foo"
version = "0.0.0"
[package]
name = "foo"
version = "1.0.0"
"#;
        let after = bump_toml(before, "1.2.3").unwrap();
        let parsed: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(parsed["workspace"]["version"].as_str(), Some("0.0.0"));
        assert_eq!(parsed["package"]["version"].as_str(), Some("1.2.3"));
    }

    #[test]
    fn errors_when_no_package_table() {
        let before = "[workspace]\nname = \"foo\"\n";
        let err = bump_toml(before, "1.2.3").unwrap_err();
        assert!(err.to_string().contains("[package]"));
    }

    #[test]
    fn errors_when_no_version_in_package_table() {
        let before = "[package]\nname = \"foo\"\n";
        let err = bump_toml(before, "1.2.3").unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn preserves_comments_within_package_table() {
        let before = r#"[package]
name = "foo"
# Bump deliberately — this comment must survive
version = "1.0.0"
description = "Test"
"#;
        let after = bump_toml(before, "1.2.3").unwrap();
        assert!(after.contains("# Bump deliberately"));
        assert!(after.contains(r#"version = "1.2.3""#));
    }
}
