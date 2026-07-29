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
