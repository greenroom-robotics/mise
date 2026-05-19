use assert_cmd::Command;
use predicates::prelude::*;

fn mise() -> Command { Command::cargo_bin("mise").unwrap() }

#[test]
fn help_lists_top_subcommands() {
    mise().arg("--help").assert().success()
        .stdout(predicate::str::contains("matrix"))
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("bump"))
        .stdout(predicate::str::contains("snapshot"));
}

#[test]
fn build_help_lists_subcommands() {
    mise().args(["build", "--help"]).assert().success()
        .stdout(predicate::str::contains("vinca"))
        .stdout(predicate::str::contains("pixi"))
        .stdout(predicate::str::contains("deepstream-container"));
}

#[test]
fn bump_help_lists_subcommands() {
    mise().args(["bump", "--help"]).assert().success()
        .stdout(predicate::str::contains("recipe"))
        .stdout(predicate::str::contains("pixi"))
        .stdout(predicate::str::contains("route"))
        .stdout(predicate::str::contains("open-pr"));
}

#[test]
fn matrix_compute_stub_bails() {
    mise().args(["matrix", "compute"]).assert().failure()
        .stderr(predicate::str::contains("not implemented"));
}

#[test]
fn build_vinca_with_args_stub_bails() {
    mise()
        .args(["build", "vinca", "--channel-url", "x", "--ds-version", "7.1"])
        .assert().failure()
        .stderr(predicate::str::contains("not implemented"));
}

#[test]
fn invalid_arch_rejected_at_parse_time() {
    mise()
        .args(["build", "vinca", "--channel-url", "x", "--target-platform", "osx-arm64"])
        .assert().failure()
        .stderr(predicate::str::contains("invalid value").or(predicate::str::contains("unknown arch")));
}

#[test]
fn invalid_deepstream_version_rejected_at_parse_time() {
    mise()
        .args(["build", "deepstream-container", "--ds-version", "9.0", "--target-platform", "linux-64"])
        .assert().failure()
        .stderr(predicate::str::contains("invalid value").or(predicate::str::contains("unknown DeepStream version")));
}
