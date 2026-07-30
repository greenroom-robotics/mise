use super::*;

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
