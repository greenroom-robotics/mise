//! `mise ci recipes-pr` characterization.
//!
//! Real temp git repos on both sides: the source repo is the process cwd (its
//! `origin` remote is only ever read, never fetched), and the recipes repo is
//! a local bare repo that a git `insteadOf` rule substitutes for the
//! tokenized GitHub clone URL, so clone/fetch/push are all real git with no
//! network. `gh` is a PATH shim: argv recorded, canned JSON replayed.
//!
//! Each routing arm of the recipes upsert is exercised end to end and the
//! resulting YAML byte-compared against a golden — format preservation is the
//! contract.

use crate::harness::{E2e, Shim, assert_golden, flag_value, package_pixi_toml, write_file};
use std::fs;
use std::path::{Path, PathBuf};

// The same constant the binary defaults to; a divergence here would make this
// suite pass against a repo slug the tool no longer uses.
use mise::consts::RECIPES_REPO;
const TOKEN: &str = "test-token";
const RELEASE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The exact URL `clone_recipes_repo` builds when GITHUB_TOKEN is set — the
/// insteadOf rule must match it byte for byte.
fn tokenized_clone_url() -> String {
    format!("https://x-access-token:{TOKEN}@github.com/{RECIPES_REPO}.git")
}

/// Build the recipes remote: seed a work repo with `files`, commit, then bare
/// clone it so pushes are accepted. Returns the bare repo path.
fn make_recipes_remote(e2e: &E2e, files: &[(&str, &str)]) -> PathBuf {
    let seed = e2e.path().join("recipes-seed");
    fs::create_dir_all(&seed).unwrap();
    for (rel, body) in files {
        write_file(&seed, rel, body);
    }
    e2e.git_init_commit(&seed, "seed recipes repo");
    let bare = e2e.path().join("recipes-remote.git");
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
    e2e.git_redirect(&tokenized_clone_url(), &bare);
    bare
}

/// Build a source repo whose root pixi.toml declares `[package] name`, with an
/// `origin` remote naming it (read for URL/short-name only). Returns its path.
fn make_source_repo(e2e: &E2e, name: &str) -> PathBuf {
    let origin = format!("https://github.com/greenroom-robotics/{name}.git");
    source_repo_with(e2e, &origin, &package_pixi_toml(name, ""), &[])
}

/// Build a monorepo-shaped source repo: workspace-only root manifest plus one
/// package per `packages` entry under `packages/<name>/`.
fn make_monorepo_source(e2e: &E2e, origin_url: &str, packages: &[&str]) -> PathBuf {
    source_repo_with(e2e, origin_url, "[workspace]\nname = \"mono\"\n", packages)
}

fn source_repo_with(
    e2e: &E2e,
    origin_url: &str,
    root_pixi_toml: &str,
    packages: &[&str],
) -> PathBuf {
    let src = e2e.path().join("source");
    fs::create_dir_all(&src).unwrap();
    write_file(&src, "pixi.toml", root_pixi_toml);
    for pkg in packages {
        write_file(
            &src,
            &format!("packages/{pkg}/pixi.toml"),
            &package_pixi_toml(pkg, ""),
        );
    }
    e2e.git_init_commit(&src, "init source");
    e2e.git(&src, &["remote", "add", "origin", origin_url]);
    src
}

/// Run `mise ci recipes-pr` from `cwd` with the standard env (token set so
/// the clone URL is the tokenized HTTPS one the redirect matches). Default gh
/// responses (no open PR; empty-success create/edit/merge; a PR URL for view)
/// are installed unless the test registered its own beforehand.
fn run_recipes_pr(
    e2e: &E2e,
    cwd: &Path,
    version: &str,
    extra_args: &[&str],
) -> assert_cmd::assert::Assert {
    e2e.respond_if_unset(Shim::Gh, &["pr", "list"], "[]");
    e2e.respond_if_unset(Shim::Gh, &["pr", "create"], "");
    e2e.respond_if_unset(Shim::Gh, &["pr", "edit"], "");
    e2e.respond_if_unset(Shim::Gh, &["pr", "merge"], "");
    e2e.respond_if_unset(
        Shim::Gh,
        &["pr", "view"],
        "https://github.com/greenroom-robotics/ros-recipes/pull/7\n",
    );
    let mut cmd = e2e.mise();
    cmd.env("GITHUB_TOKEN", TOKEN)
        .env("GITHUB_RUN_ID", "TESTRUN")
        .current_dir(cwd)
        .args([
            "ci",
            "recipes-pr",
            "--version",
            version,
            "--sha",
            RELEASE_SHA,
        ])
        .args(extra_args);
    cmd.assert()
}

/// File content at `branch:rel` in the bare remote.
fn remote_file(e2e: &E2e, remote: &Path, branch: &str, rel: &str) -> String {
    e2e.git(remote, &["show", &format!("{branch}:{rel}")])
}

/// The `<sub> <verb>` pairs of every recorded gh call, in order
/// (e.g. `["pr list", "pr create"]`).
fn gh_subcommands(e2e: &E2e) -> Vec<String> {
    e2e.shim_calls()
        .iter()
        .filter_map(|c| match c.as_slice() {
            [prog, sub, verb, ..] if prog == "gh" => Some(format!("{sub} {verb}")),
            _ => None,
        })
        .collect()
}

/// The full argv of the first recorded `gh pr <verb>` call.
fn gh_pr_call(e2e: &E2e, verb: &str) -> Vec<String> {
    e2e.shim_calls()
        .into_iter()
        .find(|c| matches!(c.as_slice(), [p, s, v, ..] if p == "gh" && s == "pr" && v == verb))
        .unwrap_or_else(|| panic!("no gh pr {verb} call recorded"))
}

const FULL_CREATE_FLOW: [&str; 4] = ["pr list", "pr create", "pr merge", "pr view"];

const PIXI_NATIVE_BASE: &str = "\
# Packages built straight from their own pixi manifests.
rebuild_epoch: 0

packages:
  - name: existing_pkg
    url: https://github.com/greenroom-robotics/existing_pkg.git
    rev: 5555555555555555555555555555555555555555
";

const ROSDISTRO_BASE: &str = "\
# Top of file comment stays.
ros_extra:
\x20 url: https://github.com/greenroom-robotics/ros_extra.git
\x20 tag: v1.0.0
\x20 version: 1.0.0

# Notes about other_entry — must survive.
other_entry:
\x20 url: https://github.com/greenroom-robotics/other_entry.git
\x20 tag: v0.1.0
\x20 version: 0.1.0
";

#[test]
fn vendored_recipe_arm_patches_recipe_and_opens_pr() {
    let e2e = E2e::new();
    // Recipe dir uses the hyphenated form of the underscore package name.
    let remote = make_recipes_remote(
        &e2e,
        &[
            (
                "vendor_recipes/demo-pkg/recipe.yaml",
                "# hand-authored recipe — comments must survive\n\
                 package:\n  name: demo-pkg\n  version: 1.0.0\n\n\
                 source:\n  git: https://github.com/greenroom-robotics/demo_pkg.git\n\
                 \x20 rev: 5555555555555555555555555555555555555555\n\n\
                 build:\n  number: 3\n  script: ${{ '$RECIPE_DIR/build.sh' }}\n",
            ),
            ("pixi_native_packages.yaml", PIXI_NATIVE_BASE),
            ("rosdistro_additional_recipes.yaml", ROSDISTRO_BASE),
        ],
    );
    let src = make_source_repo(&e2e, "demo_pkg");

    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();

    let recipe = remote_file(
        &e2e,
        &remote,
        "release/demo_pkg",
        "vendor_recipes/demo-pkg/recipe.yaml",
    );
    assert_golden(&recipe, "recipes_pr/vendored_recipe.yaml");
    // The other two routing targets were not touched.
    assert_eq!(
        remote_file(
            &e2e,
            &remote,
            "release/demo_pkg",
            "pixi_native_packages.yaml"
        ),
        PIXI_NATIVE_BASE
    );
    assert_eq!(
        remote_file(
            &e2e,
            &remote,
            "release/demo_pkg",
            "rosdistro_additional_recipes.yaml"
        ),
        ROSDISTRO_BASE
    );

    // gh flow: existence probe, create, native auto-merge, summary link.
    assert_eq!(gh_subcommands(&e2e), FULL_CREATE_FLOW);
    let create = gh_pr_call(&e2e, "create");
    assert_eq!(
        flag_value(&create, "--title"),
        Some("release: demo_pkg v2.0.0")
    );
    assert_eq!(flag_value(&create, "--base"), Some("main"));
    // Merging is native auto-merge, never a label: `--label automerge` only
    // attaches a literal label and the PR then never merges.
    assert!(!create.iter().any(|a| a == "--label"), "{create:?}");
    let merge = gh_pr_call(&e2e, "merge");
    assert!(merge.contains(&"--auto".to_string()) && merge.contains(&"--squash".to_string()));

    // The commit lands as the fallback bot identity with the run marker.
    let log = e2e.git(
        &remote,
        &["log", "-1", "--format=%an|%B", "release/demo_pkg"],
    );
    assert!(
        log.starts_with("greenroom-bot|release: demo_pkg v2.0.0"),
        "{log}"
    );
    assert!(log.contains("[mise-run:TESTRUN]"), "{log}");
}

#[test]
fn rosdistro_arm_updates_existing_entry_in_place() {
    let e2e = E2e::new();
    let remote = make_recipes_remote(
        &e2e,
        &[
            ("rosdistro_additional_recipes.yaml", ROSDISTRO_BASE),
            ("pixi_native_packages.yaml", PIXI_NATIVE_BASE),
        ],
    );
    let src = make_source_repo(&e2e, "ros_extra");

    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();

    let yaml = remote_file(
        &e2e,
        &remote,
        "release/ros_extra",
        "rosdistro_additional_recipes.yaml",
    );
    assert_golden(&yaml, "recipes_pr/rosdistro.yaml");
    // The pixi-native manifest was not touched.
    let pixi = remote_file(
        &e2e,
        &remote,
        "release/ros_extra",
        "pixi_native_packages.yaml",
    );
    assert_eq!(pixi, PIXI_NATIVE_BASE);
    assert_eq!(gh_subcommands(&e2e), FULL_CREATE_FLOW);
}

#[test]
fn pixi_native_arm_replaces_ref_with_rev_on_existing_entry() {
    let e2e = E2e::new();
    let manifest = "\
# Packages built straight from their own pixi manifests.
rebuild_epoch: 0

packages:
  - name: existing_pkg
    url: https://github.com/greenroom-robotics/existing_pkg.git
    rev: 5555555555555555555555555555555555555555

  - name: pixi_pkg
    url: https://github.com/greenroom-robotics/pixi_pkg
    ref: main
";
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", manifest)]);
    let src = make_source_repo(&e2e, "pixi_pkg");

    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();

    let yaml = remote_file(
        &e2e,
        &remote,
        "release/pixi_pkg",
        "pixi_native_packages.yaml",
    );
    assert_golden(&yaml, "recipes_pr/pixi_existing.yaml");
    assert_eq!(gh_subcommands(&e2e), FULL_CREATE_FLOW);
}

#[test]
fn brand_new_monorepo_package_appends_pixi_entry_with_subdir() {
    let e2e = E2e::new();
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", PIXI_NATIVE_BASE)]);
    // ssh origin: must be normalized to https in the recipes entry.
    let src = make_monorepo_source(
        &e2e,
        "git@github.com:greenroom-robotics/mono.git",
        &["new_pkg"],
    );

    let pkg_dir = src.join("packages");
    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", pkg_dir.to_str().unwrap()],
    )
    .success();

    // ssh origin is normalized to https, and the monorepo subdir is recorded.
    let yaml = remote_file(&e2e, &remote, "release/mono", "pixi_native_packages.yaml");
    assert_golden(&yaml, "recipes_pr/pixi_new_subdir.yaml");
}

#[test]
fn open_rolling_pr_is_appended_to_and_edited_not_created() {
    let e2e = E2e::new();
    // gh reports an open rolling PR whose body already carries a sibling.
    e2e.respond(
        Shim::Gh,
        &["pr", "list"],
        r#"[{"body":"🐉 gremlin\n\n**Releasing:**\n- other_pkg v0.9.0\n\nAutomated by `mise ci recipes-pr`."}]"#,
    );
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", PIXI_NATIVE_BASE)]);

    // The rolling branch already exists on the remote with the sibling's entry
    // pending (an earlier release of the same source repo).
    let seed2 = e2e.path().join("branch-seed");
    e2e.git(
        e2e.path(),
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            seed2.to_str().unwrap(),
        ],
    );
    e2e.git(&seed2, &["checkout", "-b", "release/mono"]);
    let pending = format!(
        "{PIXI_NATIVE_BASE}\n  - name: other_pkg\n    url: https://github.com/greenroom-robotics/mono.git\n    rev: 6666666666666666666666666666666666666666\n    subdir: packages/other_pkg\n"
    );
    write_file(&seed2, "pixi_native_packages.yaml", &pending);
    e2e.git_commit_all(&seed2, "release: mono/other_pkg v0.9.0");
    e2e.git(&seed2, &["push", "--quiet", "origin", "release/mono"]);

    let src = make_monorepo_source(
        &e2e,
        "https://github.com/greenroom-robotics/mono.git",
        &["pkg_two"],
    );

    let pkg_dir = src.join("packages");
    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", pkg_dir.to_str().unwrap()],
    )
    .success();

    // Appended on top of the pending branch content, not reset from main.
    let yaml = remote_file(&e2e, &remote, "release/mono", "pixi_native_packages.yaml");
    assert_golden(&yaml, "recipes_pr/pixi_append_branch.yaml");

    // An open PR is edited (title+body refreshed), never re-created.
    assert_eq!(
        gh_subcommands(&e2e),
        ["pr list", "pr edit", "pr merge", "pr view"]
    );

    // Characterization: the title lists packages from the PR body minus the
    // repo short-name filter — the sibling rides at its own version.
    let edit = gh_pr_call(&e2e, "edit");
    assert_eq!(
        flag_value(&edit, "--title"),
        Some("release: mono/{other_pkg v0.9.0, pkg_two v2.0.0}")
    );
}

#[test]
fn rerunning_same_release_on_open_pr_skips_commit_but_keeps_pr_fresh() {
    // Re-running the publish hook for an already-applied release (e.g. a
    // retried job): the upsert stages nothing, the commit is skipped, but the
    // branch is still pushed and the PR refreshed.
    let e2e = E2e::new();
    let manifest = "\
packages:
  - name: pixi_pkg
    url: https://github.com/greenroom-robotics/pixi_pkg
    ref: main
";
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", manifest)]);
    let src = make_source_repo(&e2e, "pixi_pkg");

    // First release creates the rolling branch + PR.
    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();
    let tip_after_first = e2e.git(&remote, &["rev-parse", "release/pixi_pkg"]);
    let gh_calls_first = gh_subcommands(&e2e).len();

    // Now the PR is open, carrying what the first run released.
    e2e.respond(
        Shim::Gh,
        &["pr", "list"],
        r#"[{"body":"**Releasing:**\n- pixi_pkg v2.0.0\n"}]"#,
    );
    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();

    // No new commit: the branch tip is byte-for-byte the first run's.
    let tip_after_second = e2e.git(&remote, &["rev-parse", "release/pixi_pkg"]);
    assert_eq!(tip_after_first, tip_after_second);

    // The second run still pushed and refreshed the PR (edit, not create).
    let second_run_calls = gh_subcommands(&e2e).split_off(gh_calls_first);
    assert_eq!(
        second_run_calls,
        ["pr list", "pr edit", "pr merge", "pr view"]
    );
}

#[test]
fn gh_pr_list_failure_is_treated_as_no_open_pr() {
    // `pr list` exiting non-zero is read as "no rolling PR": the branch is
    // reset from main and a fresh PR is created — load-bearing current
    // behavior of `pr_body_of`.
    let e2e = E2e::new();
    e2e.respond_exit(Shim::Gh, &["pr", "list"], 1);
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", PIXI_NATIVE_BASE)]);
    let src = make_source_repo(&e2e, "existing_pkg");

    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &["--package-dir", src.to_str().unwrap()],
    )
    .success();

    assert_eq!(gh_subcommands(&e2e), FULL_CREATE_FLOW);
    let yaml = remote_file(
        &e2e,
        &remote,
        "release/existing_pkg",
        "pixi_native_packages.yaml",
    );
    assert!(yaml.contains(&format!("rev: {RELEASE_SHA}")), "{yaml}");
}

#[test]
fn tolerant_sweep_skips_package_with_no_manifest_and_no_recipe() {
    let e2e = E2e::new();
    // Workspace-only root manifest: --package resolves to VendoredByName, and
    // there is no vendor_recipes/ghost_pkg in the recipes repo.
    let remote = make_recipes_remote(&e2e, &[("pixi_native_packages.yaml", PIXI_NATIVE_BASE)]);
    let src = make_monorepo_source(&e2e, "https://github.com/greenroom-robotics/mono.git", &[]);

    run_recipes_pr(
        &e2e,
        &src,
        "2.0.0",
        &[
            "--package-dir",
            src.to_str().unwrap(),
            "--package",
            "ghost_pkg",
            "--allow-missing-recipe",
        ],
    )
    .success();

    // Only the PR probe happened; nothing was pushed.
    assert_eq!(gh_subcommands(&e2e), ["pr list"]);
    let branches = e2e.git(&remote, &["branch", "--list"]);
    assert!(
        !branches.contains("release/"),
        "no release branch expected: {branches}"
    );
}
