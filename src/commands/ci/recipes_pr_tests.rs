use super::*;

#[test]
fn release_target_vendored_when_no_manifest() {
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(&pkgs).unwrap();
    // No packages/deepstream_extensions/pixi.toml and no packages/pixi.toml.
    assert_eq!(
        release_target(&pkgs, Some("deepstream_extensions")).unwrap(),
        ReleaseTarget::VendoredByName("deepstream_extensions".to_string())
    );
}

#[test]
fn release_target_discovered_when_per_package_manifest_exists() {
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(pkgs.join("object_tracker")).unwrap();
    std::fs::write(
        pkgs.join("object_tracker/pixi.toml"),
        "[package]\nname = \"object_tracker\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    assert_eq!(
        release_target(&pkgs, Some("object_tracker")).unwrap(),
        ReleaseTarget::Discovered
    );
}

#[test]
fn release_target_vendored_when_manifest_is_workspace_only() {
    // deepstream_extensions ships a dev-env pixi.toml (no [package]) so it
    // can be built with colcon in a DS container, but its conda artifact
    // still comes from vendor_recipes/. The manifest's mere existence must
    // not reclassify it as a discoverable package.
    let td = tempfile::TempDir::new().unwrap();
    let pkgs = td.path().join("packages");
    std::fs::create_dir_all(pkgs.join("deepstream_extensions")).unwrap();
    std::fs::write(
        pkgs.join("deepstream_extensions/pixi.toml"),
        "[workspace]\nname = \"deepstream_extensions\"\n[tasks]\nbuild = \"colcon build\"\n",
    )
    .unwrap();
    assert_eq!(
        release_target(&pkgs, Some("deepstream_extensions")).unwrap(),
        ReleaseTarget::VendoredByName("deepstream_extensions".to_string())
    );
}

#[test]
fn release_target_discovered_for_root_package_repo() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    std::fs::write(
        root.join("pixi.toml"),
        "[package]\nname = \"mise\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    assert_eq!(
        release_target(root, Some("mise")).unwrap(),
        ReleaseTarget::Discovered
    );
}

#[test]
fn release_target_discovered_when_no_package_filter() {
    let td = tempfile::TempDir::new().unwrap();
    assert_eq!(
        release_target(td.path(), None).unwrap(),
        ReleaseTarget::Discovered
    );
}

#[test]
fn recipe_action_applies_for_discovered() {
    // Discovered packages always apply, regardless of has_recipe/allow.
    assert_eq!(
        recipe_action(&ReleaseTarget::Discovered, false, false),
        RecipeAction::Apply
    );
}

#[test]
fn recipe_action_applies_for_vendored_with_recipe() {
    assert_eq!(
        recipe_action(&ReleaseTarget::VendoredByName("x".into()), true, false),
        RecipeAction::Apply
    );
}

#[test]
fn recipe_action_errors_for_vendored_without_recipe_when_explicit() {
    // No recipe + not allowed to miss (explicit target) -> loud error.
    assert_eq!(
        recipe_action(&ReleaseTarget::VendoredByName("x".into()), false, false),
        RecipeAction::Error
    );
}

#[test]
fn recipe_action_skips_for_vendored_without_recipe_when_sweeping() {
    // No recipe + allowed to miss (sweep) -> skip quietly.
    assert_eq!(
        recipe_action(&ReleaseTarget::VendoredByName("x".into()), false, true),
        RecipeAction::Skip
    );
}

fn pkgs(entries: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    entries
        .iter()
        .map(|(n, t)| (n.to_string(), t.to_string()))
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
        release_title("toolbox", &pkgs(&[("gama", "v1.0.0")]), "v1.0.0"),
        "release: toolbox/gama v1.0.0"
    );
    // Each package carries its own version — a rolling PR accumulates
    // packages released independently.
    assert_eq!(
        release_title(
            "toolbox",
            &pkgs(&[("gama", "v1.0.0"), ("alpha", "v0.4.1")]),
            "v1.0.0"
        ),
        "release: toolbox/{alpha v0.4.1, gama v1.0.0}"
    );
}

// The body is the rolling PR's state: every append rewrites it from what
// the previous one wrote, so it must round-trip or siblings released
// earlier vanish — along with the versions they were released at.
#[test]
fn body_packages_round_trips() {
    for names in [
        pkgs(&[]),
        pkgs(&[("gama", "v1.0.0")]),
        pkgs(&[("gama", "v1.0.0"), ("alpha", "v0.4.1")]),
    ] {
        let body = pr_body(Some("https://example.com/compare/a...b"), &names);
        assert_eq!(body_packages(&body), names, "{body}");
    }
}

// Past the title limit the list collapses to a count — but the body still
// carries every package, so the next append doesn't lose them.
#[test]
fn long_package_list_collapses_in_title_only() {
    let many = pkgs(&[
        ("gama_bringup", "v1.2.3"),
        ("gama_msgs", "v0.4.1"),
        ("gama_navigation", "v2.0.0"),
        ("topic_utils", "v1.26.0"),
    ]);
    let title = release_title("platform_toolbox", &many, "v1.2.3");
    assert_eq!(title, "release: platform_toolbox (4 packages)");
    assert!(title.chars().count() <= MAX_TITLE_CHARS);
    assert_eq!(body_packages(&pr_body(None, &many)), many);
}

// The squash-merged commit subject stays readable no matter how many
// packages ride along.
#[test]
fn title_never_exceeds_subject_limit() {
    let many: std::collections::BTreeMap<String, String> = (0..40)
        .map(|i| (format!("some_ros_package_{i}"), "v1.2.3".to_string()))
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

// Shared per-repo branch: every package of a multi-package repo lands on ONE
// rolling PR so coupled releases build together in one pr-validate run.
#[test]
fn release_branch_is_per_repo_not_per_package() {
    assert_eq!(
        release_branch("platform_toolbox"),
        "release/platform_toolbox"
    );
    assert_eq!(release_branch("mise"), "release/mise");
}

#[test]
fn diff_ref_prefers_immutable_rev_over_tag() {
    use crate::commands::ci::recipes_upsert::OldRef;
    let refs = vec![OldRef::Tag("1.2.3".into()), OldRef::Rev("deadbeef".into())];
    // Rev wins even though the tag came first.
    assert_eq!(diff_ref(&refs, "newsha"), Some("deadbeef"));
    // Tag is used when that's all there is.
    assert_eq!(
        diff_ref(&[OldRef::Tag("1.2.3".into())], "newsha"),
        Some("1.2.3")
    );
    // Same-rev re-pin and no prior pin both yield no link.
    assert_eq!(diff_ref(&[OldRef::Rev("s".into())], "s"), None);
    assert_eq!(diff_ref(&[], "s"), None);
}

#[test]
fn compare_url_strips_git_suffix() {
    assert_eq!(
        compare_url("https://github.com/gr/mise.git", "v1.0.0", "abc123"),
        "https://github.com/gr/mise/compare/v1.0.0...abc123"
    );
}

// The three-way outcome that gates commit/push/PR: only "nothing staged
// AND no open PR" is safe to bail out on before push — an open rolling PR
// means the branch carries prior pending content that must still reach
// the remote even when this package's upsert was itself a no-op.
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
    // No link line when there's no prior tag.
    assert!(!pr_body(None, &pkgs(&[])).contains("Diff since last release"));
}
