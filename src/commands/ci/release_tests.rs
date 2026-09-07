use super::*;
use crate::types::{PackageName, Version};
use clap::Parser;

fn pkg(s: &str) -> PackageName {
    PackageName::new(s).unwrap()
}

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

#[derive(Parser, Debug)]
struct TestCli {
    #[command(flatten)]
    release: Release,
}

#[test]
fn changelog_accepts_explicit_bool_value() {
    let cli = TestCli::parse_from(["x", "--package-dir", ".", "--changelog", "true"]);
    assert!(cli.release.changelog);

    let cli = TestCli::parse_from(["x", "--changelog", "false"]);
    assert!(!cli.release.changelog);
}

#[test]
fn bool_flags_default_to_true_when_omitted() {
    let cli = TestCli::parse_from(["x"]);
    assert!(cli.release.changelog);
    assert!(cli.release.github_release);
}

#[test]
fn github_release_accepts_explicit_bool_value() {
    let cli = TestCli::parse_from(["x", "--github-release", "false"]);
    assert!(!cli.release.github_release);
}

// Single-package mode must keep the same `<name>@<version>` tag convention
// as multi-package mode; a bare `${version}` format makes semantic-release
// ignore all existing `<name>@X.Y.Z` tags and restart at 1.0.0.
#[test]
fn single_package_tag_format_embeds_package_name() {
    assert_eq!(tag_format(false, &pkg("mise")), "mise@${version}");
}

#[test]
fn multi_package_tag_format_uses_msr_name_placeholder() {
    assert_eq!(tag_format(true, &pkg("mise")), "${name}@${version}");
}

#[test]
fn extra_prepare_cmd_and_git_assets_appear_in_releaserc() {
    let dir = std::env::temp_dir().join("mise-release-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"mise\"\nversion = \"1.0.0\"\n").unwrap();

    let cli = TestCli::parse_from([
        "x",
        "--extra-prepare-cmd",
        "mise ci sync-cargo --version=${nextRelease.version}",
        "--extra-git-asset",
        "Cargo.toml",
        "--extra-git-asset",
        "Cargo.lock",
    ]);
    let rc = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("mise"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let plugins = v["plugins"].as_array().unwrap();

    let exec = plugins
        .iter()
        .find(|p| p[0] == "@semantic-release/exec")
        .unwrap();
    let prepare = exec[1]["prepareCmd"].as_str().unwrap();
    assert!(prepare.contains("mise ci bump-pixi"));
    assert!(prepare.contains(" && mise ci sync-cargo --version=${nextRelease.version}"));

    let git = plugins
        .iter()
        .find(|p| p[0] == "@semantic-release/git")
        .unwrap();
    let assets = git[1]["assets"].as_array().unwrap();
    assert!(assets.iter().any(|a| a == "**/pixi.toml"));
    assert!(assets.iter().any(|a| a == "Cargo.toml"));
    assert!(assets.iter().any(|a| a == "Cargo.lock"));
}

// Versions and pins are derived from pixi.toml at the tagged rev, so the bump
// MUST be committed even with --changelog false — otherwise the tag lands on
// a rev whose manifest still carries the old version.
#[test]
fn pixi_toml_is_always_a_release_asset() {
    let dir = std::env::temp_dir().join("mise-release-asset-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
    let cli = TestCli::parse_from(["x", "--changelog", "false"]);
    let rc = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("mise"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let git = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == "@semantic-release/git")
        .expect("git plugin present even with changelog=false");
    let assets = git[1]["assets"].as_array().unwrap();
    assert!(assets.iter().any(|a| a == "**/pixi.toml"));
    assert!(!assets.iter().any(|a| a == "CHANGELOG.md"));
}

#[test]
fn git_commit_message_names_the_package() {
    let dir = std::env::temp_dir().join("mise-release-msg-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(
        &pixi,
        "[package]\nname = \"object_tracker\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let cli = TestCli::parse_from(["x"]);
    let rc = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("object_tracker"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let git = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == "@semantic-release/git")
        .unwrap()[1]
        .clone();
    let msg = git["message"].as_str().unwrap();
    assert!(
        msg.starts_with("chore(release): object_tracker ${nextRelease.version} [skip ci]"),
        "commit subject must name the package: {msg}"
    );
    assert!(
        msg.contains("${nextRelease.notes}"),
        "notes preserved: {msg}"
    );
}

#[test]
fn exec_cmd_paths_are_absolute() {
    let dir = std::env::temp_dir().join("mise-release-abs-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
    let cli = TestCli::parse_from(["x", "--package-dir", "packages"]);
    let rc = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("mise"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let exec = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == "@semantic-release/exec")
        .unwrap()[1]
        .clone();
    for key in ["prepareCmd", "publishCmd"] {
        let cmd = exec[key].as_str().unwrap();
        assert!(
            !cmd.contains("--package-dir=packages"),
            "{key} must not contain cwd-relative paths: {cmd}"
        );
        assert!(cmd.contains("--package-dir=/"), "{key} absolute: {cmd}");
    }
}

#[test]
fn prepare_cmd_runs_verify_siblings_before_bump() {
    let dir = std::env::temp_dir().join("mise-release-verify-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
    let cli = TestCli::parse_from(["x"]);
    let rc = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("mise"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let prepare = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0] == "@semantic-release/exec")
        .unwrap()[1]["prepareCmd"]
        .as_str()
        .unwrap()
        .to_string();
    let verify_pos = prepare
        .find("verify-siblings")
        .expect("verify-siblings present");
    let bump_pos = prepare.find("bump-pixi").unwrap();
    assert!(
        verify_pos < bump_pos,
        "guard must run before the bump: {prepare}"
    );
    assert!(
        !prepare.contains("pin-siblings"),
        "pin-siblings must no longer be part of prepare: {prepare}"
    );
}

#[test]
fn package_json_encodes_sibling_deps_for_msr_ordering() {
    let mut deps = std::collections::BTreeSet::new();
    deps.insert(pkg("geolocation"));
    let js = package_json_for(&pkg("geolocation_node"), &ver("1.37.0"), &deps).unwrap();
    let v: serde_json::Value = serde_json::from_str(&js).unwrap();
    assert_eq!(v["name"], "geolocation_node");
    assert_eq!(v["version"], "1.37.0");
    // NOT private: msr's ignorePrivate would skip the package outright.
    assert_eq!(v.get("private"), None);
    assert_eq!(v["dependencies"]["geolocation"], "*");
}

#[test]
fn msr_ordering_deps_encodes_path_deps_not_pins() {
    use crate::commands::ci::siblings::SiblingGraph;
    use std::collections::BTreeSet;
    let mut graph = SiblingGraph::default();
    graph
        .path_deps
        .insert(pkg("node"), BTreeSet::from([pkg("lib")]));
    graph
        .pin_deps
        .insert(pkg("node"), BTreeSet::from([pkg("msgs")]));
    let deps = msr_ordering_deps(&graph, &pkg("node"));
    assert!(deps.contains("lib"));
    assert!(!deps.contains("msgs"), "pins must not be encoded: {deps:?}");
}

#[test]
fn release_argv_multi_orders_without_cascade() {
    let argv = release_argv(true, "${name}@${version}");
    assert_eq!(argv[1], "multi-semantic-release");
    assert!(
        argv.iter().any(|a| a == "--deps.release=inherit"),
        "multi mode must suppress the cascade: {argv:?}"
    );
}

#[test]
fn release_argv_single_has_no_deps_flag() {
    let argv = release_argv(false, "v${version}");
    assert_eq!(argv[1], "semantic-release");
    assert!(!argv.iter().any(|a| a.starts_with("--deps")));
}

#[test]
fn ensure_root_workspaces_merges_into_existing_tooling_json() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join("package.json");
    std::fs::write(&root, r#"{"name":"mise-release-tooling","private":true}"#).unwrap();
    ensure_root_workspaces(&root, &["packages/geolocation".into()]).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&root).unwrap()).unwrap();
    assert_eq!(v["name"], "mise-release-tooling");
    assert_eq!(v["workspaces"][0], "packages/geolocation");
}

#[test]
fn ensure_root_workspaces_creates_minimal_json_when_absent() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join("package.json");
    ensure_root_workspaces(&root, &["packages/a".into(), "packages/b".into()]).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&root).unwrap()).unwrap();
    assert_eq!(v["private"], true);
    assert_eq!(v["workspaces"].as_array().unwrap().len(), 2);
}

#[test]
fn workspace_globs_are_cwd_relative() {
    let cwd = std::env::current_dir().unwrap();
    let abs_path = cwd.join("packages/x");
    let rel = cwd_relative(&abs_path);
    assert_eq!(rel, std::path::PathBuf::from("packages/x"));

    let rel_path = std::path::PathBuf::from("packages/y");
    let result = cwd_relative(&rel_path);
    assert_eq!(result, rel_path);
}

#[test]
fn prepend_changelog_matches_semantic_release_changelog_layout() {
    assert_eq!(
        prepend_changelog("", "## x 1.1.0\n\n* a\n"),
        "## x 1.1.0\n\n* a\n"
    );
    assert_eq!(
        prepend_changelog("## x 1.0.0\n\n* old\n", "## x 1.1.0\n\n* a"),
        "## x 1.1.0\n\n* a\n\n## x 1.0.0\n\n* old\n"
    );
}

#[test]
fn release_commit_message_names_every_package() {
    let a = Recorded {
        version: ver("1.1.0"),
        notes: "## a 1.1.0\n\n* one".into(),
    };
    let b = Recorded {
        version: ver("2.0.0"),
        notes: String::new(),
    };
    let msg = release_commit_message(&[(&pkg("a"), &a), (&pkg("b"), &b)]);
    assert_eq!(
        msg,
        "chore(release): a 1.1.0, b 2.0.0 [skip ci]\n\n## a 1.1.0\n\n* one"
    );
}

fn plugin_names(rc: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(rc).unwrap();
    v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p[0].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn record_pass_adds_record_plugin_and_no_changelog() {
    let dir = std::env::temp_dir().join("mise-release-record-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
    let cli = TestCli::parse_from(["x"]);
    let rc = cli
        .release
        .releaserc_json(&pixi, &pkg("x"), &Pass::Record { dir: &dir })
        .unwrap();
    let names = plugin_names(&rc);
    assert!(names.iter().any(|n| n.ends_with("/record_release.js")));
    assert!(!names.iter().any(|n| n == "@semantic-release/changelog"));
    let v: serde_json::Value = serde_json::from_str(&rc).unwrap();
    let record = v["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p[0].as_str().unwrap().ends_with("record_release.js"))
        .unwrap();
    assert_eq!(record[1]["name"], "x");
}

#[test]
fn release_pass_skips_changelog_plugin_once_committed() {
    let dir = std::env::temp_dir().join("mise-release-committed-test");
    std::fs::create_dir_all(&dir).unwrap();
    let pixi = dir.join("pixi.toml");
    std::fs::write(&pixi, "[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
    let cli = TestCli::parse_from(["x"]);
    let committed = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("x"),
            &Pass::Release {
                changelog_committed: true,
            },
        )
        .unwrap();
    assert!(!plugin_names(&committed).contains(&"@semantic-release/changelog".to_string()));
    let fresh = cli
        .release
        .releaserc_json(
            &pixi,
            &pkg("x"),
            &Pass::Release {
                changelog_committed: false,
            },
        )
        .unwrap();
    assert!(plugin_names(&fresh).contains(&"@semantic-release/changelog".to_string()));
}
