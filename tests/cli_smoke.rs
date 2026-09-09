#![allow(clippy::unwrap_used)]
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
        .stdout(predicate::str::contains("ci"))
        .stdout(predicate::str::contains("snapshot"));
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

// `--ros-distro` is a hidden compatibility shim on recipes-pr: the published
// composite action still passes it, but nothing reads it. Help must not
// advertise it...
#[test]
fn ci_recipes_pr_help_hides_the_ros_distro_compat_flag() {
    let out = mise()
        .args(["ci", "recipes-pr", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--recipes-repo"));
    assert!(
        !stdout.contains("--ros-distro"),
        "hidden compat flag leaked into help: {stdout}"
    );
}

// ...while still parsing, so an old action pinned against a new binary keeps
// working. An empty --package-dir bails on "no packages found" before any git
// remote, clone or `gh` call.
#[test]
fn ci_recipes_pr_still_accepts_the_ros_distro_compat_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let out = mise()
        .args([
            "ci",
            "recipes-pr",
            "--version",
            "1.2.3",
            "--sha",
            "4110a9a40736b555c7419119ef6c607951563745",
            "--ros-distro",
            "kilted",
            "--package-dir",
        ])
        .arg(tmp.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    // A clap rejection would say "unexpected argument"; reaching the
    // no-packages bail proves the flag parsed and was ignored.
    assert!(
        stderr.contains("no packages found"),
        "expected the discovery bail, got: {stderr}"
    );
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
fn ci_release_help_lists_known_flags() {
    let out = mise().args(["ci", "release", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--package"));
    assert!(stdout.contains("--package-dir"));
    assert!(stdout.contains("--recipes-repo"));
    assert!(stdout.contains("--changelog"));
    assert!(stdout.contains("--release-branches"));
}

#[test]
fn ci_recipes_pr_help_lists_known_flags() {
    let out = mise()
        .args(["ci", "recipes-pr", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("--recipes-repo"));
    assert!(stdout.contains("--package-dir"));
}

#[test]
fn ci_bump_pixi_help_lists_known_flags() {
    let out = mise().args(["ci", "bump-pixi", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("--pixi-toml"));
}

#[test]
fn ci_release_against_empty_dir_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = mise()
        .args(["ci", "release", "--package-dir"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no packages found") || stderr.contains("reading"));
}

#[test]
fn ci_recipes_pr_against_empty_dir_errors_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = mise()
        .args(["ci", "recipes-pr", "--version", "1.0.0", "--package-dir"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn ci_test_discovers_fixture_package() {
    // The `pixi run` itself fails (the fixture's env isn't installed); only
    // getting past discovery is asserted.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ci/packages");
    let out = mise()
        .args([
            "ci",
            "test",
            "--package",
            "foo",
            "--no-locked",
            "--package-dir",
        ])
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
