//! `mise build-recipes pixi` characterization.
//!
//! The early-exit paths (manifest loading, entry selection, routing
//! validation) need nothing but a fixture tree. The check/triage phase and the
//! build loop need two seams, both of which exist:
//!
//! - `MISE_GITHUB_RAW_URL` points `fetch_pixi_toml` at a local
//!   [`FixtureServer`] instead of raw.githubusercontent.com.
//! - a git `insteadOf` rule redirects `https://github.com/<owner>/<repo>` to a
//!   local bare repo, so the per-entry checkout is real git with no network.
//!
//! `pixi` stays a PATH shim: `search --json` replays a canned channel index
//! (that is what makes a package "already published"), and `publish` records
//! its argv so the build *order* can be asserted.

use crate::harness::{E2e, FixtureServer, write_file};
use predicates::prelude::*;
use std::collections::BTreeMap;
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

// ---------------------------------------------------------------------------
// check / triage, via the raw-URL and git-redirect seams
// ---------------------------------------------------------------------------

/// The channel the fixture repo builds into; only the `pixi` shim ever
/// resolves it, so the host is deliberately unroutable.
const CHANNEL: &str = "https://example.invalid/general";

/// Stand-in monorepo; a git `insteadOf` rule points it at a local bare repo.
const MONO_URL: &str = "https://github.com/example/mono.git";

/// A `pixi_native_packages.yaml` body for the given entries.
fn manifest_yaml(entries: &[(&str, &str, &str, Option<&str>)]) -> String {
    let mut out = String::from("packages:\n");
    for (name, url, rev, subdir) in entries {
        out.push_str(&format!("  - name: {name}\n"));
        out.push_str(&format!("    url: {url}\n"));
        out.push_str(&format!("    rev: {rev}\n"));
        if let Some(sub) = subdir {
            out.push_str(&format!("    subdir: {sub}\n"));
        }
    }
    out
}

/// A minimal pixi-native package manifest: builds for linux-64, not noarch,
/// with a `channels` array (which `prepend_channels` requires when a local
/// fallback channel has to be front-inserted).
fn entry_manifest(name: &str, version: &str, deps: &str) -> String {
    format!(
        "[workspace]\nname = \"{name}\"\nchannels = [\"{CHANNEL}\"]\n\
         platforms = [\"linux-64\"]\n\n\
         [package]\nname = \"{name}\"\nversion = \"{version}\"\n\n\
         [package.build]\nbackend = {{ name = \"pixi-build-cmake\" }}\n{deps}"
    )
}

/// One `pixi search --json` record, as the sweep expects to read it back.
fn search_record(name: &str, version: &str) -> String {
    format!(
        "{{\"{name}\": [{{\"name\": \"{name}\", \"version\": \"{version}\", \
         \"build_number\": 0, \"subdir\": \"linux-64\"}}]}}"
    )
}

/// The `--path` argument of every recorded `pixi publish`, in call order.
fn publish_paths(e2e: &E2e) -> Vec<String> {
    e2e.shim_calls()
        .iter()
        .filter(|c| matches!(c.as_slice(), [p, v, ..] if p == "pixi" && v == "publish"))
        .filter_map(|c| crate::harness::flag_value(c, "--path").map(str::to_string))
        .collect()
}

/// The subpath of `--path` after the temp checkout root, which is the only
/// stable part (the tempdir name is random).
fn manifest_subpath(path: &str) -> &str {
    path.rsplit_once("/src/")
        .map(|(_, rest)| rest)
        .unwrap_or(path)
}

#[test]
fn already_published_entry_is_triaged_out_and_never_built() {
    let e2e = E2e::new();
    let rev = "2222222222222222222222222222222222222222";
    let root = repo_with_manifest(
        &e2e,
        &manifest_yaml(&[("alpha", "https://github.com/example/alpha.git", rev, None)]),
    );
    let raw = FixtureServer::start(BTreeMap::from([(
        format!("/example/alpha/{rev}/pixi.toml"),
        entry_manifest("alpha", "1.0.0", ""),
    )]));
    // The channel already holds alpha 1.0.0 build 0 for linux-64 — exactly the
    // artifact this job would produce.
    e2e.respond(
        crate::harness::Shim::Pixi,
        &["search", "--json"],
        &search_record("alpha", "1.0.0"),
    );

    let mut cmd = e2e.mise();
    cmd.env("MISE_GITHUB_RAW_URL", raw.base_url());
    let assert = cmd
        .args([
            "build-recipes",
            "pixi",
            "--repo-root",
            root.to_str().unwrap(),
            "--channel-url",
            CHANNEL,
            "--output-dir",
        ])
        .arg(e2e.path().join("out"))
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("skipping alpha 1.0.0: already in channel(s)"),
        "{stderr}"
    );
    assert!(stderr.contains("nothing to build"), "{stderr}");
    // The channel was swept, but nothing was published and no checkout happened.
    assert!(publish_paths(&e2e).is_empty(), "{:?}", e2e.shim_calls());
}

#[test]
fn unpublished_entries_build_in_dependency_order() {
    let e2e = E2e::new();

    // A two-package monorepo where `app` path-depends on `lib`. Served as a
    // local bare repo standing in for github.com/example/mono.
    let seed = e2e.path().join("mono-seed");
    fs::create_dir_all(&seed).unwrap();
    write_file(&seed, "lib/pixi.toml", &entry_manifest("lib", "1.0.0", ""));
    write_file(
        &seed,
        "app/pixi.toml",
        &entry_manifest(
            "app",
            "2.0.0",
            "\n[dependencies]\nlib = { path = \"../lib\" }\n",
        ),
    );
    let rev = e2e.git_init_commit(&seed, "seed mono");
    let bare = e2e.path().join("mono.git");
    e2e.git(
        e2e.path(),
        &[
            "clone",
            "--bare",
            "--quiet",
            seed.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    e2e.git_redirect(MONO_URL.trim_end_matches(".git"), &bare);

    // Entries listed consumer-first on purpose: the topological sort, not the
    // manifest order, has to decide the build order.
    let root = repo_with_manifest(
        &e2e,
        &manifest_yaml(&[
            ("app", MONO_URL, &rev, Some("app")),
            ("lib", MONO_URL, &rev, Some("lib")),
        ]),
    );
    let raw = FixtureServer::start(BTreeMap::from([
        (
            format!("/example/mono/{rev}/lib/pixi.toml"),
            entry_manifest("lib", "1.0.0", ""),
        ),
        (
            format!("/example/mono/{rev}/app/pixi.toml"),
            entry_manifest(
                "app",
                "2.0.0",
                "\n[dependencies]\nlib = { path = \"../lib\" }\n",
            ),
        ),
    ]));
    // Empty channel: nothing is published, so both entries need building.
    e2e.respond(crate::harness::Shim::Pixi, &["search", "--json"], "{}");
    e2e.respond(crate::harness::Shim::Pixi, &["publish", "--path"], "");

    let mut cmd = e2e.mise();
    cmd.env("MISE_GITHUB_RAW_URL", raw.base_url());
    let assert = cmd
        .args([
            "build-recipes",
            "pixi",
            "--repo-root",
            root.to_str().unwrap(),
            "--channel-url",
            CHANNEL,
            "--output-dir",
        ])
        .arg(e2e.path().join("out"))
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("building 2 entries"), "{stderr}");

    let paths = publish_paths(&e2e);
    let order: Vec<&str> = paths.iter().map(|p| manifest_subpath(p)).collect();
    assert_eq!(
        order,
        ["lib/pixi.toml", "app/pixi.toml"],
        "dependency target must build first; got {paths:?}"
    );
}

// An unreachable channel used to look identical to an empty one: every package
// appeared unpublished and the job rebuilt and republished the lot. It has to
// be a hard failure instead.
#[test]
fn an_unreachable_channel_fails_the_job_instead_of_rebuilding_everything() {
    let e2e = E2e::new();
    let rev = "2222222222222222222222222222222222222222";
    let root = repo_with_manifest(
        &e2e,
        &manifest_yaml(&[("alpha", "https://github.com/example/alpha.git", rev, None)]),
    );
    let raw = FixtureServer::start(BTreeMap::from([(
        format!("/example/alpha/{rev}/pixi.toml"),
        entry_manifest("alpha", "1.0.0", ""),
    )]));
    e2e.respond_stderr(
        crate::harness::Shim::Pixi,
        &["search", "--json"],
        "  × error sending request for url (https://example.invalid/general/noarch/repodata.json)\n",
    );
    e2e.respond_exit(crate::harness::Shim::Pixi, &["search", "--json"], 1);

    let mut cmd = e2e.mise();
    cmd.env("MISE_GITHUB_RAW_URL", raw.base_url());
    cmd.args([
        "build-recipes",
        "pixi",
        "--repo-root",
        root.to_str().unwrap(),
        "--channel-url",
        CHANNEL,
        "--output-dir",
    ])
    .arg(e2e.path().join("out"))
    .assert()
    .failure()
    .stderr(predicate::str::contains("could not reach"));

    assert!(publish_paths(&e2e).is_empty(), "{:?}", e2e.shim_calls());
}
