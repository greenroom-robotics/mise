//! `mise ci release` characterization: the command's observable output is the
//! `.releaserc` / `package.json` files it synthesizes and the `npx` argv it
//! hands to (multi-)semantic-release. `npx` is a shim, so nothing actually
//! releases.

use crate::harness::{E2e, Shim, assert_golden, normalize_path, package_pixi_toml, write_file};
use std::fs;

#[test]
fn single_package_release_writes_releaserc_and_runs_semantic_release() {
    let e2e = E2e::new();
    e2e.respond(Shim::Npx, &[], "");
    let src = e2e.path().join("source");
    fs::create_dir_all(&src).unwrap();
    write_file(&src, "pixi.toml", &package_pixi_toml("solo", ""));

    e2e.mise()
        .current_dir(&src)
        .args(["ci", "release", "--package-dir", src.to_str().unwrap()])
        .assert()
        .success();

    // Exactly one npx invocation, in single-package mode with the package
    // name embedded literally in the tag format.
    assert_eq!(
        e2e.shim_calls(),
        vec![vec![
            "npx".to_string(),
            "--no-install".to_string(),
            "semantic-release".to_string(),
            "--tag-format=solo@${version}".to_string(),
        ]]
    );

    // The synthesized .releaserc, with the temp path normalized, is golden.
    let releaserc = fs::read_to_string(src.join(".releaserc")).unwrap();
    assert_golden(
        &normalize_path(&releaserc, &src),
        "release/single_package.releaserc.json",
    );
    // Single-package mode synthesizes no package.json files.
    assert!(!src.join("package.json").exists());
}

#[test]
fn multi_package_release_synthesizes_workspaces_and_ordering_deps() {
    let e2e = E2e::new();
    e2e.respond(Shim::Npx, &[], "");
    let src = e2e.path().join("source");
    fs::create_dir_all(&src).unwrap();
    write_file(&src, "pixi.toml", "[workspace]\nname = \"mono\"\n");
    write_file(
        &src,
        "packages/liba/pixi.toml",
        &package_pixi_toml("liba", ""),
    );
    write_file(
        &src,
        "packages/nodeb/pixi.toml",
        &package_pixi_toml(
            "nodeb",
            "\n[package.run-dependencies]\nliba = { path = \"../liba\" }\n",
        ),
    );
    let pkg_dir = src.join("packages");

    e2e.mise()
        .current_dir(&src)
        .args(["ci", "release", "--package-dir", pkg_dir.to_str().unwrap()])
        .assert()
        .success();

    // Multi mode: multi-semantic-release with the ordering-only deps flag.
    assert_eq!(
        e2e.shim_calls(),
        vec![vec![
            "npx".to_string(),
            "--no-install".to_string(),
            "multi-semantic-release".to_string(),
            "--tag-format=${name}@${version}".to_string(),
            "--deps.release=inherit".to_string(),
        ]]
    );

    // Root package.json lists cwd-relative workspace globs.
    let root_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(src.join("package.json")).unwrap()).unwrap();
    assert_eq!(
        root_json["workspaces"],
        serde_json::json!(["packages/liba", "packages/nodeb"])
    );

    // Per-package package.json: path dep encoded as "*" for msr ordering.
    let nodeb: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(src.join("packages/nodeb/package.json")).unwrap())
            .unwrap();
    assert_eq!(nodeb["name"], "nodeb");
    assert_eq!(nodeb["dependencies"]["liba"], "*");
    let liba: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(src.join("packages/liba/package.json")).unwrap())
            .unwrap();
    assert_eq!(liba["dependencies"], serde_json::json!({}));

    // Each package got a .releaserc whose publish step targets that package.
    let rc = fs::read_to_string(src.join("packages/nodeb/.releaserc")).unwrap();
    assert_golden(
        &normalize_path(&rc, &src),
        "release/multi_nodeb.releaserc.json",
    );
}
