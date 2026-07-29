//! `mise build-recipes pixi` characterization — the slice reachable without
//! network.
//!
//! The check/triage phase proper cannot be exercised end to end today: its
//! first step (`fetch_pixi_toml`) is an in-process HTTPS request to
//! raw.githubusercontent.com with no seam a PATH shim can intercept, so any
//! entry that survives selection would hit the network. What IS pinned here is
//! everything that happens before that fan-out: manifest loading, entry
//! selection, and routing-rule validation — including the guarantee that no
//! `pixi` subprocess is spawned on those paths.

use crate::harness::{E2e, write_file};
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_with_manifest(e2e: &E2e, manifest_yaml: &str) -> PathBuf {
    let root = e2e.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    write_file(&root, "pixi.toml", "[workspace]\nname = \"recipes\"\n");
    write_file(&root, "pixi_native_packages.yaml", manifest_yaml);
    root
}

fn run_pixi(e2e: &E2e, root: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = e2e.mise();
    cmd.args([
        "build-recipes",
        "pixi",
        "--repo-root",
        root.to_str().unwrap(),
        "--channel-url",
        "https://example.invalid/general",
        "--output-dir",
    ])
    .arg(e2e.path().join("out"))
    .args(extra);
    cmd.assert()
}

const ONE_ENTRY: &str = "\
packages:
  - name: alpha
    url: https://github.com/example/alpha.git
    rev: 2222222222222222222222222222222222222222
";

#[test]
fn empty_manifest_is_a_successful_noop() {
    let e2e = E2e::new();
    let root = repo_with_manifest(&e2e, "packages: []\n");
    run_pixi(&e2e, &root, &[]).success();
    // No channel sweep, no pixi subprocess at all.
    assert!(e2e.shim_calls().is_empty());
}

#[test]
fn runner_size_filter_selecting_nothing_exits_before_any_channel_work() {
    let e2e = E2e::new();
    // alpha defaults to 4cpu; asking for 16cpu selects nothing.
    let root = repo_with_manifest(&e2e, ONE_ENTRY);
    run_pixi(&e2e, &root, &["--runner-size", "16cpu"]).success();
    assert!(e2e.shim_calls().is_empty());
}

#[test]
fn only_filter_selecting_nothing_exits_before_any_channel_work() {
    let e2e = E2e::new();
    let root = repo_with_manifest(&e2e, ONE_ENTRY);
    run_pixi(&e2e, &root, &["--only", "no-such-package"]).success();
    assert!(e2e.shim_calls().is_empty());
}

#[test]
fn malformed_routing_yaml_fails_before_any_check() {
    let e2e = E2e::new();
    let root = repo_with_manifest(&e2e, ONE_ENTRY);
    write_file(&root, "routing.yaml", "rules: notalist\n");
    run_pixi(&e2e, &root, &[])
        .failure()
        .stderr(predicate::str::contains("routing.yaml"));
    // Routing is validated before the check fan-out: no pixi call happened.
    assert!(e2e.shim_calls().is_empty());
}
