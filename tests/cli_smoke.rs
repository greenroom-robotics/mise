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
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("bump"))
        .stdout(predicate::str::contains("snapshot"));
}

#[test]
fn build_help_lists_subcommands() {
    mise()
        .args(["build", "--help"])
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
fn build_vinca_rejects_ds_version_without_recipe() {
    // VincaBuildMode::from_flags rejects --ds-version without any --ds-recipe.
    // The command fails before any subprocess is spawned.
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    mise()
        .args([
            "build",
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
fn build_pixi_fails_when_manifest_missing() {
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("pixi.toml"), "[workspace]\nname=\"x\"\n").unwrap();
    // No pixi_native_packages.yaml in the temp repo — should fail at load.
    mise()
        .args([
            "build",
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
            "build",
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
fn invalid_deepstream_version_rejected_at_parse_time() {
    mise()
        .args([
            "build",
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
