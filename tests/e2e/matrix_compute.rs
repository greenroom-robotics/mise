//! `mise matrix compute` characterization: fixture event payloads + fixture
//! repo trees, golden-asserting the full `$GITHUB_OUTPUT` contents.

use crate::harness::{E2e, FixtureServer, assert_golden, write_file};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A synthetic head SHA for push events (never resolved — the publish-SHA
/// rebase degrades before any git use of it).
const FAKE_SHA: &str = "1111111111111111111111111111111111111111";

/// Lay down the fixture ros-recipes-like tree `matrix compute` reads:
/// root pixi.toml, the pixi-native manifest (alpha@4cpu, bravo@16cpu), two
/// deepstream recipes and two deepstream versions. The recipes are listed
/// out of sorted order deliberately: `recipes-csv` joins the `BTreeSet` in
/// `DeepstreamCfg` (src/repo.rs), so the output must come back sorted
/// (`deepstream-thing,zeta-thing`) regardless of listing order in
/// `.github/deepstream-recipes.yaml`.
fn matrix_repo(e2e: &E2e) -> PathBuf {
    let root = e2e.path().join("repo");
    fs::create_dir_all(&root).unwrap();
    write_file(&root, "pixi.toml", "[workspace]\nname = \"recipes\"\n");
    write_file(
        &root,
        "pixi_native_packages.yaml",
        "# pixi-native packages\npackages:\n\
         \x20 - name: alpha\n\
         \x20   url: https://github.com/example/alpha.git\n\
         \x20   rev: 2222222222222222222222222222222222222222\n\
         \x20 - name: bravo\n\
         \x20   url: https://github.com/example/bravo.git\n\
         \x20   rev: 3333333333333333333333333333333333333333\n\
         \x20   runner-size: 16cpu\n",
    );
    write_file(
        &root,
        ".github/deepstream-recipes.yaml",
        "recipes:\n  - zeta-thing\n  - deepstream-thing\n",
    );
    write_file(
        &root,
        "variants/deepstream.yaml",
        "deepstream_version:\n  - \"7.1\"\n  - \"8.0\"\n",
    );
    root
}

struct MatrixRun {
    output: String,
    stdout: String,
}

/// Run `matrix compute` against `root` with the given event, capturing the
/// full `$GITHUB_OUTPUT` file.
fn run_matrix(e2e: &E2e, root: &Path, event_name: &str, event_json: Option<&str>) -> MatrixRun {
    run_matrix_with(e2e, root, event_name, event_json, &[])
}

/// As [`run_matrix`], plus extra environment variables.
fn run_matrix_with(
    e2e: &E2e,
    root: &Path,
    event_name: &str,
    event_json: Option<&str>,
    extra_env: &[(&str, &str)],
) -> MatrixRun {
    let out_file = e2e.path().join("github_output");
    fs::write(&out_file, "").unwrap();
    let event_path = e2e.path().join("event.json");
    if let Some(json) = event_json {
        fs::write(&event_path, json).unwrap();
    }

    let mut cmd = e2e.mise();
    cmd.env("GITHUB_EVENT_NAME", event_name)
        .env("GITHUB_OUTPUT", &out_file)
        .env("GITHUB_RUN_ID", "TESTRUN")
        .args(["matrix", "compute", "--repo-root"])
        .arg(root);
    if event_json.is_some() {
        cmd.env("GITHUB_EVENT_PATH", &event_path);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let assert = cmd.assert().success();
    let out = assert.get_output();
    MatrixRun {
        output: fs::read_to_string(&out_file).unwrap(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// A repo with two commits: `mutate` edits the tree for the head commit.
/// Returns (root, base_sha, head_sha).
fn pr_repo(e2e: &E2e, mutate: impl FnOnce(&Path)) -> (PathBuf, String, String) {
    let root = matrix_repo(e2e);
    let base = e2e.git_init_commit(&root, "base");
    mutate(&root);
    let head = e2e.git_commit_all(&root, "head");
    (root, base, head)
}

fn pr_event(base: &str, head: &str) -> String {
    format!(r#"{{"pull_request":{{"base":{{"sha":"{base}"}},"head":{{"sha":"{head}"}}}}}}"#)
}

#[test]
fn workflow_dispatch_builds_everything() {
    let e2e = E2e::new();
    let root = matrix_repo(&e2e);

    let run = run_matrix(&e2e, &root, "workflow_dispatch", None);
    assert_golden(&run.output, "matrix/workflow_dispatch.github_output.txt");
    // The matrix JSON is also printed to stdout for log visibility.
    assert!(run.stdout.contains("\"include\""), "stdout: {}", run.stdout);
}

#[test]
fn schedule_event_builds_everything() {
    // Unknown event names degrade to ChangedFiles::All (full rebuild).
    let e2e = E2e::new();
    let root = matrix_repo(&e2e);

    let run = run_matrix(&e2e, &root, "schedule", Some("{}"));
    assert_golden(&run.output, "matrix/schedule.github_output.txt");
}

#[test]
fn push_without_publish_lookup_env_degrades_to_full_rebuild() {
    // Push events rebase `before` onto the last successful publish run, which
    // needs GITHUB_REPOSITORY + a token. Without them the lookup fails and
    // change detection degrades to a full rebuild — the event's own `before`
    // SHA is discarded either way.
    let e2e = E2e::new();
    let root = matrix_repo(&e2e);

    let event = format!(r#"{{"before":"{FAKE_SHA}","after":"{FAKE_SHA}"}}"#);
    let run = run_matrix(&e2e, &root, "push", Some(&event));
    assert_golden(&run.output, "matrix/push_no_token.github_output.txt");
}

#[test]
fn pr_touching_only_docs_yields_no_work() {
    let e2e = E2e::new();
    let (root, base, head) = pr_repo(&e2e, |root| {
        write_file(root, "README.md", "docs only\n");
    });
    let run = run_matrix(&e2e, &root, "pull_request", Some(&pr_event(&base, &head)));
    assert_golden(&run.output, "matrix/pr_docs_only.github_output.txt");
    assert!(run.output.contains("has-work=false"));
}

#[test]
fn pr_touching_vinca_yaml_triggers_vinca_and_deepstream() {
    let e2e = E2e::new();
    let (root, base, head) = pr_repo(&e2e, |root| {
        write_file(root, "vinca.yaml", "packages: []\n");
    });
    let run = run_matrix(&e2e, &root, "pull_request", Some(&pr_event(&base, &head)));
    assert_golden(&run.output, "matrix/pr_vinca_global.github_output.txt");
}

#[test]
fn pr_changing_one_manifest_entry_scopes_pixi_to_it() {
    // Bumping alpha's rev in pixi_native_packages.yaml scopes pixi-native work
    // to alpha's runner size only, and names it in the pixi-only output.
    let e2e = E2e::new();
    let (root, base, head) = pr_repo(&e2e, |root| {
        let manifest = root.join("pixi_native_packages.yaml");
        let text = fs::read_to_string(&manifest).unwrap().replace(
            "2222222222222222222222222222222222222222",
            "4444444444444444444444444444444444444444",
        );
        fs::write(&manifest, text).unwrap();
    });
    let run = run_matrix(&e2e, &root, "pull_request", Some(&pr_event(&base, &head)));
    assert_golden(
        &run.output,
        "matrix/pr_manifest_rev_change.github_output.txt",
    );
    assert!(run.output.contains("pixi-only=alpha"));
}

#[test]
fn pr_comment_only_manifest_change_yields_no_pixi_work() {
    // The manifest file changed but no package entry did: the scoped diff
    // resolves to nothing to build.
    let e2e = E2e::new();
    let (root, base, head) = pr_repo(&e2e, |root| {
        let manifest = root.join("pixi_native_packages.yaml");
        let text = fs::read_to_string(&manifest)
            .unwrap()
            .replace("# pixi-native packages", "# pixi-native packages (edited)");
        fs::write(&manifest, text).unwrap();
    });
    let run = run_matrix(&e2e, &root, "pull_request", Some(&pr_event(&base, &head)));
    assert_golden(
        &run.output,
        "matrix/pr_manifest_comment_only.github_output.txt",
    );
    assert!(run.output.contains("has-work=false"));
}

#[test]
fn push_rebases_change_detection_onto_the_last_successful_publish() {
    // The success path of the publish-SHA lookup, reachable only through the
    // MISE_GITHUB_API_URL seam. The event's own `before` is a dead SHA that
    // git could not diff from; the API answer (the real base commit) is what
    // change detection must use, scoping the run to the one entry that moved.
    let e2e = E2e::new();
    let (root, base, head) = pr_repo(&e2e, |root| {
        let manifest = root.join("pixi_native_packages.yaml");
        let text = fs::read_to_string(&manifest).unwrap().replace(
            "2222222222222222222222222222222222222222",
            "4444444444444444444444444444444444444444",
        );
        fs::write(&manifest, text).unwrap();
    });

    let api = FixtureServer::start(BTreeMap::from([(
        "/repos/example/recipes/actions/workflows/publish.yml/runs".to_string(),
        format!(r#"{{"workflow_runs":[{{"head_sha":"{base}"}}]}}"#),
    )]));

    let event = format!(r#"{{"before":"{FAKE_SHA}","after":"{head}"}}"#);
    let run = run_matrix_with(
        &e2e,
        &root,
        "push",
        Some(&event),
        &[
            ("GITHUB_REPOSITORY", "example/recipes"),
            ("GITHUB_TOKEN", "test-token"),
            ("MISE_GITHUB_API_URL", api.base_url()),
        ],
    );

    assert_golden(
        &run.output,
        "matrix/push_rebased_to_publish.github_output.txt",
    );
    assert!(run.output.contains("pixi-only=alpha"), "{}", run.output);
}

#[test]
fn push_with_no_prior_publish_run_degrades_to_full_rebuild() {
    // An empty `workflow_runs` list means the channel has never been
    // published: there is no base to diff from, so everything rebuilds.
    let e2e = E2e::new();
    let (root, _base, head) = pr_repo(&e2e, |root| {
        write_file(root, "README.md", "docs only\n");
    });

    let api = FixtureServer::start(BTreeMap::from([(
        "/repos/example/recipes/actions/workflows/publish.yml/runs".to_string(),
        r#"{"workflow_runs":[]}"#.to_string(),
    )]));

    let event = format!(r#"{{"before":"{FAKE_SHA}","after":"{head}"}}"#);
    let run = run_matrix_with(
        &e2e,
        &root,
        "push",
        Some(&event),
        &[
            ("GITHUB_REPOSITORY", "example/recipes"),
            ("GITHUB_TOKEN", "test-token"),
            ("MISE_GITHUB_API_URL", api.base_url()),
        ],
    );

    // Identical to the no-token degradation path: full rebuild.
    assert_golden(&run.output, "matrix/push_no_token.github_output.txt");
}
