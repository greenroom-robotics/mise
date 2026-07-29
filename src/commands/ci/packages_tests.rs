use super::*;
use std::fs;
use tempfile::TempDir;

fn make_pkg(root: &Path, name: &str) {
    let pkg = root.join(name);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        format!(
            "[workspace]\nname = \"{name}\"\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"
        ),
    )
    .unwrap();
}

/// A dev-environment manifest: workspace, no `[package]`.
fn make_workspace_only(root: &Path, name: &str) {
    let pkg = root.join(name);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        format!("[workspace]\nname = \"{name}\"\n[tasks]\nbuild = \"colcon build\"\n"),
    )
    .unwrap();
}

#[test]
fn discovers_all_packages_when_no_filter() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_pkg(tmp.path(), "beta");
    let mut result = discover(tmp.path(), None).unwrap();
    result.sort();
    assert_eq!(result.len(), 2);
    assert!(result[0].ends_with("alpha/pixi.toml"));
    assert!(result[1].ends_with("beta/pixi.toml"));
}

#[test]
fn discover_with_filter_returns_single_package() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_pkg(tmp.path(), "beta");
    let result = discover(tmp.path(), Some("alpha")).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_filter_unknown_package_errors() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let err = discover(tmp.path(), Some("ghost")).unwrap_err();
    assert!(err.to_string().contains("ghost"));
}

#[test]
fn discover_returns_root_package_when_pixi_declares_package() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("pixi.toml"),
        "[workspace]\nname = \"mise\"\n[package]\nname = \"mise\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("pixi.toml"));
    // Filter matching the root package name also returns it.
    let filtered = discover(tmp.path(), Some("mise")).unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
fn discover_falls_through_when_root_pixi_is_workspace_only() {
    // A workspace-only root pixi.toml must NOT shadow real subdir packages.
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("pixi.toml"), "[workspace]\nname = \"ws\"\n").unwrap();
    make_pkg(tmp.path(), "alpha");
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_skips_workspace_only_manifest() {
    // deepstream_extensions shape: a dev env for a package published from a
    // hand-authored recipe. It must not break the whole repo's release.
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_workspace_only(tmp.path(), "deepstream_extensions");
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_filter_on_workspace_only_manifest_errors() {
    let tmp = TempDir::new().unwrap();
    make_workspace_only(tmp.path(), "devenv");
    let err = discover(tmp.path(), Some("devenv")).unwrap_err();
    assert!(err.to_string().contains("no [package] section"));
}

#[test]
fn discover_propagates_malformed_manifest() {
    // A syntax error is a real problem — don't silently skip the package.
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("broken");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("pixi.toml"), "[workspace\nname = ").unwrap();
    let err = discover(tmp.path(), None).unwrap_err();
    assert!(format!("{err:#}").contains("parsing"));
}

#[test]
fn discover_skips_directories_without_pixi_toml() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    std::fs::create_dir_all(tmp.path().join("not-a-package")).unwrap();
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
}
