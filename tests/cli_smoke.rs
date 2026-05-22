use assert_cmd::Command;
use predicates::prelude::*;

fn mise() -> Command {
    Command::cargo_bin("mise").unwrap()
}

#[test]
fn help_lists_top_subcommands() {
    mise()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("matrix"))
        .stdout(predicate::str::contains("build-recipes"))
        .stdout(predicate::str::contains("bump"))
        .stdout(predicate::str::contains("snapshot"));
}

#[test]
fn build_recipes_help_lists_subcommands() {
    mise()
        .args(["build-recipes", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("vinca"))
        .stdout(predicate::str::contains("pixi"))
        .stdout(predicate::str::contains("deepstream-container"));
}

#[test]
fn bump_help_lists_subcommands() {
    mise()
        .args(["bump", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("recipe"))
        .stdout(predicate::str::contains("pixi"))
        .stdout(predicate::str::contains("route"))
        .stdout(predicate::str::contains("open-pr"));
}

#[test]
fn matrix_compute_workflow_dispatch_produces_matrix() {
    use std::fs;
    use std::io::Read;
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();

    fs::write(root.join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    fs::write(
        root.join("pixi_native_packages.yaml"),
        "packages:\n  - name: a\n    url: https://github.com/x/y.git\n    rev: 4110a9a40736b555c7419119ef6c607951563745\n",
    ).unwrap();
    fs::create_dir_all(root.join(".github")).unwrap();
    fs::write(
        root.join(".github/deepstream-recipes.yaml"),
        "recipes:\n  - deepstream-x\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("variants")).unwrap();
    fs::write(
        root.join("variants/deepstream.yaml"),
        "deepstream_version:\n  - \"7.1\"\n  - \"8.0\"\n",
    )
    .unwrap();

    let out_file = tempfile::NamedTempFile::new().unwrap();

    let assert = mise()
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_OUTPUT", out_file.path())
        .env("GITHUB_RUN_ID", "TEST")
        .args(["matrix", "compute", "--repo-root", root.to_str().unwrap()])
        .assert()
        .success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"include\""), "stdout: {stdout}");
    assert!(
        stdout.contains("\"pipeline\":\"vinca\""),
        "stdout: {stdout}"
    );

    let mut s = String::new();
    fs::File::open(out_file.path())
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s.contains("has-work=true"), "$GITHUB_OUTPUT: {s}");
    assert!(s.contains("matrix-json="), "$GITHUB_OUTPUT: {s}");
    assert!(
        s.contains("recipes-csv=deepstream-x"),
        "$GITHUB_OUTPUT: {s}"
    );
}

#[test]
fn build_recipes_vinca_rejects_ds_version_without_recipe() {
    // VincaBuildMode::from_flags rejects --ds-version without any --ds-recipe.
    // The command fails before any subprocess is spawned.
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    mise()
        .args([
            "build-recipes",
            "vinca",
            "--repo-root",
            td.path().to_str().unwrap(),
            "--channel-url",
            "https://example.com/channel",
            "--ds-version",
            "7.1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "requires at least one --ds-recipe",
        ));
}

#[test]
fn build_recipes_pixi_fails_when_manifest_missing() {
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    // No pixi_native_packages.yaml in the temp repo — should fail at load.
    mise()
        .args([
            "build-recipes",
            "pixi",
            "--repo-root",
            td.path().to_str().unwrap(),
            "--channel-url",
            "https://example.com/channel",
        ])
        .assert()
        .failure();
}

#[test]
fn invalid_arch_rejected_at_parse_time() {
    mise()
        .args([
            "build-recipes",
            "vinca",
            "--channel-url",
            "x",
            "--target-platform",
            "osx-arm64",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid value").or(predicate::str::contains("unknown arch")),
        );
}

#[test]
fn build_recipes_deepstream_container_requires_recipe() {
    mise()
        .args([
            "build-recipes",
            "deepstream-container",
            "--channel-url",
            "https://example.com/channel",
            "--ds-version",
            "7.1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--ds-recipe"));
}

#[test]
fn bump_recipe_errors_when_package_missing() {
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    std::fs::write(
        td.path().join("rosdistro_additional_recipes.yaml"),
        "foo:\n  tag: 1.0\n  version: 1.0\n",
    )
    .unwrap();
    mise()
        .args([
            "bump",
            "recipe",
            "--repo-root",
            td.path().to_str().unwrap(),
            "missing-pkg",
            "1.0.0",
            "0000000000000000000000000000000000000000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing-pkg"));
}

#[test]
fn invalid_deepstream_version_rejected_at_parse_time() {
    mise()
        .args([
            "build-recipes",
            "deepstream-container",
            "--ds-version",
            "9.0",
            "--target-platform",
            "linux-64",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid value")
                .or(predicate::str::contains("unknown DeepStream version")),
        );
}

#[test]
fn bump_open_pr_fails_when_payload_missing() {
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    mise()
        .args([
            "bump",
            "open-pr",
            "--repo-root",
            td.path().to_str().unwrap(),
            "--payload",
            "/nonexistent/payload.json",
        ])
        .assert()
        .failure();
}

#[test]
fn build_recipes_pixi_rejects_ref_entries_in_manifest() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    std::fs::write(root.join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    std::fs::write(
        root.join("pixi_native_packages.yaml"),
        "packages:\n  - name: foo\n    url: https://github.com/x/y.git\n    ref: main\n",
    )
    .unwrap();

    mise()
        .args([
            "build-recipes",
            "pixi",
            "--repo-root",
            root.to_str().unwrap(),
            "--channel-url",
            "https://example.invalid/channel",
            "--output-dir",
            td.path().join("out").to_str().unwrap(),
            "--target-platform",
            "linux-64",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no longer supported"))
        .stderr(predicate::str::contains("foo"));
}

#[test]
fn ci_help_lists_test_and_build() {
    let out = mise().args(["ci", "--help"]).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("test"));
    assert!(stdout.contains("build"));
}

#[test]
fn ci_test_help_lists_known_flags() {
    let out = mise().args(["ci", "test", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--package"));
    assert!(stdout.contains("--package-dir"));
    assert!(stdout.contains("--ros-distro"));
}

#[test]
fn ci_build_help_lists_known_flags() {
    let out = mise().args(["ci", "build", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--package"));
    assert!(stdout.contains("--package-dir"));
    assert!(stdout.contains("--target-platform"));
}

#[test]
fn ci_test_against_empty_dir_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = mise()
        .args(["ci", "test", "--package-dir"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no packages found") || stderr.contains("reading"));
}

#[test]
fn ci_test_discovers_fixture_package() {
    // Discovery should find `foo`; the actual `pixi run` will fail since the
    // fixture's tests env isn't installed during cargo test, but mise should
    // get past discovery and attempt the run.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ci/packages");
    let out = mise()
        .args(["ci", "test", "--package", "foo", "--package-dir"])
        .arg(&fixture)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("mise ci test :: ") && stdout.contains("foo"),
        "expected discovery banner in stdout. stdout={stdout} stderr={stderr}"
    );
}
