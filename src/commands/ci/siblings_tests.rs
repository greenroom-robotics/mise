use super::*;
use std::fs;
use tempfile::TempDir;

fn write_pkg(root: &std::path::Path, name: &str, extra: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    let body = format!(
        "[workspace]\nname = \"{name}\"\n\n[dependencies]\n{name} = {{ path = \".\" }}\n\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{extra}"
    );
    let p = dir.join("pixi.toml");
    fs::write(&p, body).unwrap();
    p
}

#[test]
fn detects_path_dep_between_siblings() {
    let tmp = TempDir::new().unwrap();
    let a = write_pkg(tmp.path(), "geolocation", "");
    let b = write_pkg(
        tmp.path(),
        "geolocation_node",
        "[package.run-dependencies]\ngeolocation = { path = \"../geolocation\" }\n",
    );
    let g = analyze(&[a, b]).unwrap();
    assert!(g.path_deps["geolocation_node"].contains("geolocation"));
    assert!(g.path_deps.get("geolocation").is_none_or(|s| s.is_empty()));
}

#[test]
fn self_path_dep_idiom_is_ignored() {
    let tmp = TempDir::new().unwrap();
    let a = write_pkg(tmp.path(), "solo", "");
    let g = analyze(&[a]).unwrap();
    assert!(g.path_deps.get("solo").is_none_or(|s| s.is_empty()));
}

#[test]
fn detects_version_pin_on_sibling() {
    let tmp = TempDir::new().unwrap();
    let a = write_pkg(tmp.path(), "geolocation", "");
    let b = write_pkg(
        tmp.path(),
        "geolocation_node",
        "[package.run-dependencies]\ngeolocation = \"==1.0.0\"\n",
    );
    let g = analyze(&[a, b]).unwrap();
    assert!(g.pin_deps["geolocation_node"].contains("geolocation"));
}

#[test]
fn host_dependencies_also_scanned() {
    let tmp = TempDir::new().unwrap();
    let a = write_pkg(tmp.path(), "geolocation_msgs", "");
    let b = write_pkg(
        tmp.path(),
        "geolocation_node",
        "[package.host-dependencies]\ngeolocation_msgs = { path = \"../geolocation_msgs\" }\n",
    );
    let g = analyze(&[a, b]).unwrap();
    assert!(g.path_deps["geolocation_node"].contains("geolocation_msgs"));
}

#[test]
fn external_deps_produce_no_edges() {
    let tmp = TempDir::new().unwrap();
    let a = write_pkg(
        tmp.path(),
        "geolocation",
        "[package.run-dependencies]\nros-kilted-rclpy = \"*\"\npydantic = \">=2,<3\"\n",
    );
    let g = analyze(&[a]).unwrap();
    assert!(g.pin_deps.get("geolocation").is_none_or(|s| s.is_empty()));
}

#[test]
fn package_xml_mode_manifest_falls_back_to_package_xml_name() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("geolocation");
    fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    fs::write(
        &pixi,
        "[workspace]\nname = \"geolocation\"\n\n[package]\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    fs::write(
            dir.join("package.xml"),
            "<?xml version=\"1.0\"?>\n<package format=\"3\">\n  <name>geolocation</name>\n  <version>1.0.0</version>\n</package>\n",
        )
        .unwrap();
    let g = analyze(&[pixi]).unwrap();
    assert!(g.dirs.contains_key("geolocation"));
}

#[test]
fn missing_name_in_both_manifest_and_package_xml_errors_mentioning_both() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("geolocation");
    fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    fs::write(
        &pixi,
        "[workspace]\nname = \"geolocation\"\n\n[package]\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let err = analyze(&[pixi]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("package.name"));
    assert!(msg.contains("package.xml"));
}
