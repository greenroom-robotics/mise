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
