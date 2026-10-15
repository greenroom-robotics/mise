use super::*;
use crate::types::Version;

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn manifest(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n")
}

#[test]
fn bumps_a_single_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pixi.toml");
    std::fs::write(&path, manifest("pkg_a", "1.0.0")).unwrap();
    BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![path.clone()],
    }
    .run()
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        manifest("pkg_a", "2.0.0")
    );
}

#[test]
fn bumps_several_manifests_to_the_same_version() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.toml");
    let b = tmp.path().join("b.toml");
    std::fs::write(&a, manifest("pkg_a", "1.0.0")).unwrap();
    std::fs::write(&b, manifest("pkg_b", "0.3.1")).unwrap();
    BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![a.clone(), b.clone()],
    }
    .run()
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        manifest("pkg_a", "2.0.0")
    );
    assert_eq!(
        std::fs::read_to_string(&b).unwrap(),
        manifest("pkg_b", "2.0.0")
    );
}

#[test]
fn missing_manifest_fails_before_any_write() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.toml");
    std::fs::write(&a, manifest("pkg_a", "1.0.0")).unwrap();
    let err = BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![a.clone(), tmp.path().join("missing.toml")],
    }
    .run()
    .unwrap_err();
    assert!(err.to_string().contains("missing.toml"));
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        manifest("pkg_a", "1.0.0")
    );
}

#[test]
fn invalid_manifest_fails_before_any_write() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.toml");
    let b = tmp.path().join("b.toml");
    std::fs::write(&a, manifest("pkg_a", "1.0.0")).unwrap();
    std::fs::write(&b, "[workspace]\nname = \"not_a_package\"\n").unwrap();
    let err = BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![a.clone(), b],
    }
    .run()
    .unwrap_err();
    assert!(err.to_string().contains("b.toml"));
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        manifest("pkg_a", "1.0.0")
    );
}

#[test]
fn bumps_cargo_files_beside_each_package() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("pkgs/foo");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, manifest("foo", "1.0.0")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), manifest("foo", "1.0.0")).unwrap();
    std::fs::write(
        dir.join("Cargo.lock"),
        "version = 3\n\n[[package]]\nname = \"foo\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![pixi],
    }
    .run()
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
        manifest("foo", "2.0.0")
    );
    assert!(
        std::fs::read_to_string(dir.join("Cargo.lock"))
            .unwrap()
            .contains("version = \"2.0.0\"")
    );
}

const TOOL_LOCK: &str = "version = 3\n\n[[package]]\nname = \"tool-cli\"\nversion = \"1.0.0\"\n";
const WORKSPACE_TOML: &str = "[workspace]\nmembers = [\"tool-cli\"]\n";

fn pixi_with_build(dir: &std::path::Path, build: &str) -> PathBuf {
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, format!("{}{build}", manifest("tool", "1.0.0"))).unwrap();
    pixi
}

fn bump(pixi: PathBuf) -> color_eyre::eyre::Result<()> {
    BumpPixi {
        version: ver("2.0.0"),
        pixi_toml: vec![pixi],
    }
    .run()
}

#[test]
fn bumps_subcrate_named_by_source_path() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("tool-cli")).unwrap();
    let pixi = pixi_with_build(dir, "\n[package.build]\nsource.path = \"tool-cli\"\n");
    std::fs::write(dir.join("Cargo.toml"), WORKSPACE_TOML).unwrap();
    std::fs::write(dir.join("Cargo.lock"), TOOL_LOCK).unwrap();
    std::fs::write(
        dir.join("tool-cli/Cargo.toml"),
        manifest("tool-cli", "1.0.0"),
    )
    .unwrap();
    bump(pixi).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("tool-cli/Cargo.toml")).unwrap(),
        manifest("tool-cli", "2.0.0")
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("Cargo.lock")).unwrap(),
        TOOL_LOCK.replace("1.0.0", "2.0.0")
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
        WORKSPACE_TOML
    );
}

#[test]
fn virtual_workspace_without_source_path_errors_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pixi = pixi_with_build(dir, "");
    std::fs::write(dir.join("Cargo.toml"), WORKSPACE_TOML).unwrap();
    let err = bump(pixi).unwrap_err();
    assert!(format!("{err:#}").contains("Cargo.toml"));
    assert_eq!(
        std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
        WORKSPACE_TOML
    );
}

#[test]
fn non_local_source_touches_no_cargo_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let pixi = pixi_with_build(
        dir,
        "\n[package.build]\nsource.git = \"https://example.com/tool\"\n",
    );
    std::fs::write(dir.join("Cargo.toml"), manifest("tool", "1.0.0")).unwrap();
    bump(pixi).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("Cargo.toml")).unwrap(),
        manifest("tool", "1.0.0")
    );
}
