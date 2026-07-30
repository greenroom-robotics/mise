use super::*;
use tempfile::TempDir;

use crate::types::Version;

fn pkg(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

fn ver(v: &str) -> Version {
    Version::parse(v).unwrap()
}

fn write_checkout_pkg(root: &Path, name: &str, extra: &str) -> PathBuf {
    let dir = root.join("packages").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("pixi.toml");
    std::fs::write(
        &p,
        format!(
            "[workspace]\nname = \"{name}\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
                 [dependencies]\n{name} = {{ path = \".\" }}\n\
                 [package]\nname = \"{name}\"\nversion = \"2.5.0\"\n{extra}"
        ),
    )
    .unwrap();
    p
}

/// dep-name -> subdir map for `resolve_sibling_pins`, matching how the main
/// build loop in `super::super::pixi` derives it from same-repo
/// `manifest.packages` entries.
fn subdirs(pairs: &[(&str, &str)]) -> BTreeMap<PackageName, PathBuf> {
    pairs
        .iter()
        .map(|(n, d)| (pkg(n), PathBuf::from(d)))
        .collect()
}

#[test]
fn resolve_sibling_pins_resolves_matching_pin() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", ""); // version 2.5.0
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = \"==2.5.0\"\n",
    );
    let map = subdirs(&[("lib", "packages/lib")]);
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name.as_str(), "lib");
    assert_eq!(resolved[0].version, ver("2.5.0"));
    assert_eq!(resolved[0].manifest, root.join("packages/lib/pixi.toml"));
}

#[test]
fn resolve_sibling_pins_skips_version_mismatch() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", ""); // checkout is 2.5.0
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = \"==2.4.0\"\n", // older pin
    );
    let map = subdirs(&[("lib", "packages/lib")]);
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert!(resolved.is_empty(), "older pin left to the real channel");
}

#[test]
fn resolve_sibling_pins_ignores_non_sibling_pins() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nros-kilted-rclpy = \"==1.0.0\"\n",
    );
    let map = subdirs(&[("lib", "packages/lib")]); // rclpy not in map
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert!(resolved.is_empty());
}

#[test]
fn check_local_build_guard_detects_cycle() {
    let visiting = vec![pkg("a"), pkg("b")];
    let local_built = BTreeSet::new();
    let err = check_local_build_guard(&pkg("a"), &visiting, &local_built).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cycle"), "message: {msg}");
    assert!(msg.contains("a -> b -> a"), "message: {msg}");
}

#[test]
fn check_local_build_guard_skips_already_built() {
    let visiting: Vec<PackageName> = Vec::new();
    let mut local_built = BTreeSet::new();
    local_built.insert(pkg("lib"));
    assert!(!check_local_build_guard(&pkg("lib"), &visiting, &local_built).unwrap());
    // Not yet built and not visiting: proceed.
    assert!(check_local_build_guard(&pkg("other"), &visiting, &local_built).unwrap());
}
