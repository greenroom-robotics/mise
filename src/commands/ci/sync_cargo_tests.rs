use super::*;
use crate::types::Version;
use std::path::{Path, PathBuf};

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
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
    let after = bump_cargo_lock(before, "mise", &ver("2.3.4")).unwrap();
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
    let err = bump_cargo_lock(before, "mise", &ver("2.3.4")).unwrap_err();
    assert!(err.to_string().contains("mise"));
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn cargo_toml(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")
}

fn lock_versions(path: &Path) -> Vec<(String, String)> {
    let doc: toml_edit::DocumentMut = std::fs::read_to_string(path).unwrap().parse().unwrap();
    doc["package"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().to_owned(),
                p["version"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

const LOCK: &str = r#"version = 3

[[package]]
name = "foo"
version = "1.0.0"

[[package]]
name = "other"
version = "1.0.0"

[[package]]
name = "foo"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

fn assert_foo_only_bumped(lock: &Path) {
    assert_eq!(
        lock_versions(lock),
        vec![
            ("foo".to_owned(), "2.0.0".to_owned()),
            ("other".to_owned(), "1.0.0".to_owned()),
            ("foo".to_owned(), "1.0.0".to_owned()),
        ]
    );
}

#[test]
fn syncs_crate_and_lock_in_package_dir() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "Cargo.toml", &cargo_toml("foo", "1.0.0"));
    write(tmp.path(), "Cargo.lock", LOCK);
    let touched = sync_package(tmp.path(), &ver("2.0.0")).unwrap();
    assert_eq!(
        touched,
        vec![tmp.path().join("Cargo.toml"), tmp.path().join("Cargo.lock")]
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap(),
        cargo_toml("foo", "2.0.0")
    );
    assert_foo_only_bumped(&tmp.path().join("Cargo.lock"));
}

#[test]
fn syncs_lock_found_in_ancestor_workspace_root() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "ws/Cargo.lock", LOCK);
    write(
        tmp.path(),
        "ws/crates/foo/Cargo.toml",
        &cargo_toml("foo", "1.0.0"),
    );
    let touched = sync_package(&tmp.path().join("ws/crates/foo"), &ver("2.0.0")).unwrap();
    assert_eq!(touched.len(), 2);
    assert_foo_only_bumped(&tmp.path().join("ws/Cargo.lock"));
}

#[test]
fn crate_without_lock_only_touches_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "Cargo.toml", &cargo_toml("foo", "1.0.0"));
    let touched = sync_package(tmp.path(), &ver("2.0.0")).unwrap();
    assert_eq!(touched, vec![tmp.path().join("Cargo.toml")]);
}

#[test]
fn workspace_inherited_version_errors_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let toml = "[package]\nname = \"foo\"\nversion.workspace = true\n";
    write(tmp.path(), "Cargo.toml", toml);
    write(tmp.path(), "Cargo.lock", LOCK);
    let err = sync_package(tmp.path(), &ver("2.0.0")).unwrap_err();
    assert!(format!("{err:#}").contains("Cargo.toml"));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap(),
        toml
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.lock")).unwrap(),
        LOCK
    );
}

#[test]
fn missing_lock_entry_errors_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let toml = cargo_toml("absent", "1.0.0");
    write(tmp.path(), "Cargo.toml", &toml);
    write(tmp.path(), "Cargo.lock", LOCK);
    assert!(sync_package(tmp.path(), &ver("2.0.0")).is_err());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap(),
        toml
    );
}

#[test]
fn non_cargo_dir_is_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "Cargo.lock", LOCK);
    assert_eq!(
        sync_package(tmp.path(), &ver("2.0.0")).unwrap(),
        Vec::<PathBuf>::new()
    );
    assert_eq!(cargo_files(tmp.path()).unwrap(), Vec::<PathBuf>::new());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.lock")).unwrap(),
        LOCK
    );
}

#[test]
fn virtual_workspace_manifest_errors_naming_file_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let toml = "[workspace]\nmembers = []\n";
    write(tmp.path(), "Cargo.toml", toml);
    write(tmp.path(), "Cargo.lock", LOCK);
    let err = sync_package(tmp.path(), &ver("2.0.0")).unwrap_err();
    assert!(format!("{err:#}").contains("Cargo.toml"));
    assert!(cargo_files(tmp.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap(),
        toml
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("Cargo.lock")).unwrap(),
        LOCK
    );
}

#[test]
fn lock_search_stops_at_git_root() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "Cargo.lock", LOCK);
    write(tmp.path(), "repo/.git/HEAD", "");
    write(
        tmp.path(),
        "repo/foo/Cargo.toml",
        &cargo_toml("foo", "1.0.0"),
    );
    let dir = tmp.path().join("repo/foo");
    assert_eq!(cargo_files(&dir).unwrap(), vec![dir.join("Cargo.toml")]);
}
