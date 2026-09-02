use super::*;

fn pkg(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

fn sha(hex_digit: char) -> Sha40 {
    Sha40::new(std::iter::repeat_n(hex_digit, 40).collect::<String>()).unwrap()
}

#[test]
fn release_mode_vendored_when_no_manifest() {
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(&pkgs).unwrap();
    assert_eq!(
        release_mode(&pkgs, Some(&pkg("deepstream_extensions"))).unwrap(),
        ReleaseMode::VendoredByName(pkg("deepstream_extensions"))
    );
}

#[test]
fn release_mode_discovered_when_per_package_manifest_exists() {
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(pkgs.join("object_tracker")).unwrap();
    std::fs::write(
        pkgs.join("object_tracker/pixi.toml"),
        "[package]\nname = \"object_tracker\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    assert_eq!(
        release_mode(&pkgs, Some(&pkg("object_tracker"))).unwrap(),
        ReleaseMode::Discovered
    );
}

#[test]
fn release_mode_vendored_when_manifest_is_workspace_only() {
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(pkgs.join("deepstream_extensions")).unwrap();
    std::fs::write(
        pkgs.join("deepstream_extensions/pixi.toml"),
        "[workspace]\nname = \"deepstream_extensions\"\n[tasks]\nbuild = \"colcon build\"\n",
    )
    .unwrap();
    assert_eq!(
        release_mode(&pkgs, Some(&pkg("deepstream_extensions"))).unwrap(),
        ReleaseMode::VendoredByName(pkg("deepstream_extensions"))
    );
}

#[test]
fn release_mode_discovered_for_root_package_repo() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    std::fs::write(
        root.join("pixi.toml"),
        "[package]\nname = \"mise\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    assert_eq!(
        release_mode(root, Some(&pkg("mise"))).unwrap(),
        ReleaseMode::Discovered
    );
}

#[test]
fn release_mode_discovered_when_no_package_filter() {
    let td = tempfile::TempDir::new().unwrap();
    assert_eq!(
        release_mode(td.path(), None).unwrap(),
        ReleaseMode::Discovered
    );
}

#[test]
fn recipe_action_applies_for_discovered() {
    assert_eq!(
        recipe_action(&ReleaseMode::Discovered, false, false),
        RecipeAction::Apply
    );
}

#[test]
fn recipe_action_applies_for_vendored_with_recipe() {
    assert_eq!(
        recipe_action(&ReleaseMode::VendoredByName(pkg("x")), true, false),
        RecipeAction::Apply
    );
}

#[test]
fn recipe_action_errors_for_vendored_without_recipe_when_explicit() {
    assert_eq!(
        recipe_action(&ReleaseMode::VendoredByName(pkg("x")), false, false),
        RecipeAction::Error
    );
}

#[test]
fn recipe_action_skips_for_vendored_without_recipe_when_sweeping() {
    assert_eq!(
        recipe_action(&ReleaseMode::VendoredByName(pkg("x")), false, true),
        RecipeAction::Skip
    );
}

fn pkgs(entries: &[(&str, &str)]) -> std::collections::BTreeMap<PackageName, String> {
    entries
        .iter()
        .map(|(n, t)| (pkg(n), t.to_string()))
        .collect()
}

#[test]
fn release_title_shapes() {
    assert_eq!(
        release_title("mise", &pkgs(&[]), "v1.0.0"),
        "release: mise v1.0.0"
    );
    // A root package sharing the repo name isn't a subpath.
    assert_eq!(
        release_title("mise", &pkgs(&[("mise", "v1.0.0")]), "v1.0.0"),
        "release: mise v1.0.0"
    );
    assert_eq!(
        release_title("toolbox", &pkgs(&[("beta", "v1.0.0")]), "v1.0.0"),
        "release: toolbox/beta v1.0.0"
    );
    assert_eq!(
        release_title(
            "toolbox",
            &pkgs(&[("beta", "v1.0.0"), ("alpha", "v0.4.1")]),
            "v1.0.0"
        ),
        "release: toolbox/{alpha v0.4.1, beta v1.0.0}"
    );
}

// The body is the rolling PR's state: every append rewrites it from what
// the previous one wrote, so it must round-trip or siblings released
// earlier vanish — along with the versions they were released at.
#[test]
fn body_packages_round_trips() {
    for names in [
        pkgs(&[]),
        pkgs(&[("beta", "v1.0.0")]),
        pkgs(&[("beta", "v1.0.0"), ("alpha", "v0.4.1")]),
    ] {
        let body = pr_body(Some("https://example.com/compare/a...b"), &names);
        assert_eq!(body_packages(&body), names, "{body}");
    }
}

#[test]
fn long_package_list_collapses_in_title_only() {
    let many = pkgs(&[
        ("beta_bringup", "v1.2.3"),
        ("beta_msgs", "v0.4.1"),
        ("beta_navigation", "v2.0.0"),
        ("topic_utils", "v1.26.0"),
    ]);
    let title = release_title("toolbox_repo", &many, "v1.2.3");
    assert_eq!(title, "release: toolbox_repo (4 packages)");
    assert!(title.chars().count() <= MAX_TITLE_CHARS);
    assert_eq!(body_packages(&pr_body(None, &many)), many);
}

#[test]
fn title_never_exceeds_subject_limit() {
    let many: std::collections::BTreeMap<PackageName, String> = (0..40)
        .map(|i| (pkg(&format!("some_ros_package_{i}")), "v1.2.3".to_string()))
        .collect();
    assert!(release_title("toolbox", &many, "v1.2.3").chars().count() <= MAX_TITLE_CHARS);
}

// The rolling-PR contract: the branch name must NOT embed the version, so
// every release of a source repo force-pushes onto the same branch and
// updates one PR. A per-version branch leaves superseded PRs open and lets
// an older release merge over a newer one.
#[test]
fn release_branch_is_version_independent() {
    let a = release_branch("mise");
    assert_eq!(a, "release/mise");
    assert!(!a.contains("4.5"), "branch must not embed a version: {a}");
}

#[test]
fn release_branch_is_per_repo_not_per_package() {
    assert_eq!(release_branch("toolbox_repo"), "release/toolbox_repo");
    assert_eq!(release_branch("mise"), "release/mise");
}

#[test]
fn diff_ref_prefers_immutable_rev_over_tag() {
    use crate::commands::ci::recipes_upsert::OldRef;
    let new = sha('b');
    let old = sha('a');
    let refs = vec![
        OldRef::Tag("1.2.3".into()),
        OldRef::Rev(old.as_str().into()),
    ];
    // Rev wins even though the tag came first.
    assert_eq!(diff_ref(&refs, &new), Some(old.as_str()));
    assert_eq!(
        diff_ref(&[OldRef::Tag("1.2.3".into())], &new),
        Some("1.2.3")
    );
    assert_eq!(diff_ref(&[OldRef::Rev(new.as_str().into())], &new), None);
    assert_eq!(diff_ref(&[], &new), None);
}

#[test]
fn compare_url_has_no_git_suffix() {
    let repo = GithubRepoUrl::parse_remote("https://github.com/gr/mise.git").unwrap();
    assert_eq!(
        repo.compare_url("v1.0.0", "abc123"),
        "https://github.com/gr/mise/compare/v1.0.0...abc123"
    );
}

#[test]
fn classify_noop_outcomes() {
    assert_eq!(classify_noop(false, false), NoopOutcome::Commit);
    assert_eq!(classify_noop(false, true), NoopOutcome::Commit);
    assert_eq!(classify_noop(true, true), NoopOutcome::SkipCommitKeepPush);
    assert_eq!(classify_noop(true, false), NoopOutcome::EarlyReturn);
}

#[test]
fn pr_body_includes_diff_link_when_present() {
    let body = pr_body(
        Some("https://github.com/gr/mise/compare/v1.0.0...v1.1.0"),
        &pkgs(&[]),
    );
    assert!(body.contains("Diff since last release"));
    assert!(body.contains("compare/v1.0.0...v1.1.0"));
    assert!(!pr_body(None, &pkgs(&[])).contains("Diff since last release"));
}
