use super::*;

#[test]
fn pipeline_serializes_to_kebab_case() {
    assert_eq!(
        serde_json::to_string(&Pipeline::Vinca).unwrap(),
        "\"vinca\""
    );
    assert_eq!(
        serde_json::to_string(&Pipeline::PixiNative).unwrap(),
        "\"pixi-native\""
    );
    assert_eq!(
        serde_json::to_string(&Pipeline::ShouldNotRun).unwrap(),
        "\"should-not-run\""
    );
}

#[test]
fn matrix_entry_serializes_with_kebab_case_keys() {
    let e = MatrixEntry {
        pipeline: Pipeline::Vinca,
        target_platform: Arch::Linux64,
        ds_version: String::new(),
        ds_image: String::new(),
        runner: "runs-on=1/runner=4cpu-linux-x64".into(),
        runner_size: String::new(),
        artifact_name: "build-linux-64".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert!(
        json.contains("\"target-platform\":\"linux-64\""),
        "got: {json}"
    );
    assert!(json.contains("\"ds-version\":\"\""), "got: {json}");
    assert!(json.contains("\"runner-size\":\"\""), "got: {json}");
    assert!(
        json.contains("\"artifact-name\":\"build-linux-64\""),
        "got: {json}"
    );
}

use crate::types::{DeepstreamVersion, RecipeName};
use std::str::FromStr;

fn cfg(recipes: &[&str], versions: &[DeepstreamVersion]) -> DeepstreamCfg {
    DeepstreamCfg {
        recipes: recipes
            .iter()
            .map(|n| RecipeName::from_str(n).unwrap())
            .collect(),
        versions: versions.iter().copied().collect(),
    }
}

fn paths(ps: &[&str]) -> ChangedFiles {
    ChangedFiles::Paths(ps.iter().map(std::path::PathBuf::from).collect())
}

#[test]
fn classify_all_means_everything() {
    let ds = cfg(
        &["foo"],
        &[DeepstreamVersion::V7_1, DeepstreamVersion::V8_0],
    );
    let s = classify(&ChangedFiles::All, &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::All);
    assert_eq!(s.ds_versions.len(), 2);
}

#[test]
fn classify_global_vinca_triggers_vinca_and_ds() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vinca.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V7_1));
}

#[test]
fn classify_global_both_triggers_everything() {
    let ds = cfg(
        &["foo"],
        &[DeepstreamVersion::V7_1, DeepstreamVersion::V8_0],
    );
    let s = classify(&paths(&["pixi.toml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::All);
    assert_eq!(s.ds_versions.len(), 2);
}

#[test]
fn classify_workflow_prefix_triggers_everything() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&[".github/workflows/build.yml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::All);
    assert!(!s.ds_versions.is_empty());
}

#[test]
fn classify_deepstream_recipes_file_triggers_vinca_and_ds() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&[".github/deepstream-recipes.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V7_1));
}

#[test]
fn classify_variants_file_triggers_only_ds() {
    let ds = cfg(&[], &[DeepstreamVersion::V8_0]);
    let s = classify(&paths(&["variants/deepstream.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V8_0));
}

#[test]
fn classify_rosdistro_additional_triggers_vinca_only() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["rosdistro_additional_recipes.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_pixi_native_manifest_triggers_pixi_only() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi_native_packages.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, PixiScope::ManifestScoped);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_vendor_recipes_ds_match_triggers_ds_only() {
    let ds = cfg(&["my-ds-recipe"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vendor_recipes/my-ds-recipe/recipe.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V7_1));
}

#[test]
fn classify_vendor_recipes_non_ds_triggers_vinca() {
    let ds = cfg(&["other-recipe"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vendor_recipes/regular-recipe/recipe.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, PixiScope::None);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_recipes_path_is_ignored() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["recipes/some-generated/recipe.yaml"]), &ds);
    assert_eq!(s, MatrixState::default());
}

#[test]
fn classify_unrelated_paths_yield_no_work() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["README.md", "docs/some-doc.md"]), &ds);
    assert_eq!(s, MatrixState::default());
}

use crate::types::{PixiNativeEntry, PixiNativeManifest, Sha40};

fn empty_manifest() -> PixiNativeManifest {
    PixiNativeManifest {
        rebuild_epoch: 0,
        packages: vec![],
    }
}

fn manifest_with_sizes(sizes: &[RunnerSize]) -> PixiNativeManifest {
    let url = crate::types::GithubRepoUrl::parse("https://github.com/x/y").unwrap();
    let sha = Sha40::new("4110a9a40736b555c7419119ef6c607951563745").unwrap();
    let packages = sizes
        .iter()
        .enumerate()
        .map(|(i, size)| PixiNativeEntry {
            name: format!("pkg{i}"),
            url: url.clone(),
            rev: sha.clone(),
            subdir: None,
            runner_size: *size,
        })
        .collect();
    PixiNativeManifest {
        rebuild_epoch: 0,
        packages,
    }
}

#[test]
fn build_matrix_empty_state_yields_nothing() {
    let state = MatrixState::default();
    let out = build_matrix(&state, &empty_manifest(), "RUN");
    assert!(out.is_empty());
}

#[test]
fn build_matrix_vinca_only_produces_two_arches() {
    let state = MatrixState {
        vinca: true,
        ..Default::default()
    };
    let out = build_matrix(&state, &empty_manifest(), "RUN");
    assert_eq!(out.len(), 2);
    assert!(
        out.iter()
            .all(|e| e.pipeline == Pipeline::Vinca && e.ds_version.is_empty())
    );
    assert!(out.iter().any(|e| e.target_platform == Arch::Linux64));
    assert!(out.iter().any(|e| e.target_platform == Arch::LinuxAarch64));
    assert!(out[0].runner.contains("RUN"));
}

#[test]
fn build_matrix_pixi_native_groups_by_size() {
    let state = MatrixState {
        pixi_native: PixiScope::All,
        ..Default::default()
    };
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4, RunnerSize::Cpu4, RunnerSize::Cpu8]);
    let out = build_matrix(&state, &manifest, "RUN");
    // Two unique sizes (4, 8) × 2 arches = 4 entries
    assert_eq!(out.len(), 4);
    assert!(out.iter().all(|e| e.pipeline == Pipeline::PixiNative));
    let mut sizes: Vec<&str> = out.iter().map(|e| e.runner_size.as_str()).collect();
    sizes.sort();
    sizes.dedup();
    assert_eq!(sizes, vec!["4cpu", "8cpu"]);
}

#[test]
fn build_matrix_pixi_native_uses_correct_arch_tag() {
    let state = MatrixState {
        pixi_native: PixiScope::All,
        ..Default::default()
    };
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4]);
    let out = build_matrix(&state, &manifest, "RUN");
    let x64 = out
        .iter()
        .find(|e| e.target_platform == Arch::Linux64)
        .unwrap();
    let arm = out
        .iter()
        .find(|e| e.target_platform == Arch::LinuxAarch64)
        .unwrap();
    assert!(x64.runner.contains("4cpu-linux-x64"));
    assert!(arm.runner.contains("4cpu-linux-arm64"));
}

#[test]
fn build_matrix_ds_versions_produce_per_arch_rows() {
    let mut state = MatrixState::default();
    state.ds_versions.insert(DeepstreamVersion::V7_1);
    state.ds_versions.insert(DeepstreamVersion::V8_0);
    let out = build_matrix(&state, &empty_manifest(), "RUN");
    assert_eq!(out.len(), 4);
    let v71_x64 = out
        .iter()
        .find(|e| e.ds_version == "7.1" && e.target_platform == Arch::Linux64)
        .unwrap();
    assert_eq!(
        v71_x64.ds_image,
        "nvcr.io/nvidia/deepstream:7.1-triton-multiarch"
    );
    assert!(v71_x64.runner.contains("family=c6id.xlarge"));
    assert!(v71_x64.runner.contains("deepstream-x64-7.1"));
    let v80_arm = out
        .iter()
        .find(|e| e.ds_version == "8.0" && e.target_platform == Arch::LinuxAarch64)
        .unwrap();
    assert!(v80_arm.runner.contains("family=c7gd.xlarge"));
    assert!(v80_arm.runner.contains("deepstream-arm64-8.0"));
}

#[test]
fn build_matrix_ds_versions_sorted_ascending() {
    let mut state = MatrixState::default();
    state.ds_versions.insert(DeepstreamVersion::V8_0);
    state.ds_versions.insert(DeepstreamVersion::V7_1);
    let out = build_matrix(&state, &empty_manifest(), "RUN");
    // First 2 entries should be 7.1, last 2 should be 8.0
    assert_eq!(out[0].ds_version, "7.1");
    assert_eq!(out[1].ds_version, "7.1");
    assert_eq!(out[2].ds_version, "8.0");
    assert_eq!(out[3].ds_version, "8.0");
}

#[test]
fn placeholder_entry_is_should_not_run() {
    let e = placeholder_entry("RUN");
    assert_eq!(e.pipeline, Pipeline::ShouldNotRun);
    assert_eq!(e.artifact_name, "should-not-run");
    assert!(e.runner.contains("RUN"));
}

#[test]
fn diff_detects_added_changed_and_ignores_removed() {
    let base = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
  - name: beta
    url: https://github.com/org/beta
    rev: 2222222222222222222222222222222222222222
  - name: gone
    url: https://github.com/org/gone
    rev: 3333333333333333333333333333333333333333
"#;
    let head = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
  - name: beta
    url: https://github.com/org/beta
    rev: 9999999999999999999999999999999999999999
  - name: added
    url: https://github.com/org/added
    rev: 4444444444444444444444444444444444444444
"#;
    let changed = diff_changed_packages(Some(base), head).unwrap();
    assert_eq!(
        changed,
        ["added".to_string(), "beta".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn diff_detects_runner_size_change() {
    let base = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
"#;
    let head = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
    runner-size: 16cpu
"#;
    let changed = diff_changed_packages(Some(base), head).unwrap();
    assert_eq!(changed, ["alpha".to_string()].into_iter().collect());
}

#[test]
fn diff_none_base_means_all_head_packages() {
    let head = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
  - name: beta
    url: https://github.com/org/beta
    rev: 2222222222222222222222222222222222222222
"#;
    let changed = diff_changed_packages(None, head).unwrap();
    assert_eq!(
        changed,
        ["alpha".to_string(), "beta".to_string()]
            .into_iter()
            .collect()
    );
}

#[test]
fn classify_manifest_only_is_scoped() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi_native_packages.yaml"]), &ds);
    assert_eq!(s.pixi_native, PixiScope::ManifestScoped);
    assert!(!s.vinca);
}

#[test]
fn classify_global_both_forces_pixi_all() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi.toml", "pixi_native_packages.yaml"]), &ds);
    assert_eq!(s.pixi_native, PixiScope::All);
}

#[test]
fn classify_changedfiles_all_is_pixi_all() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&ChangedFiles::All, &ds);
    assert_eq!(s.pixi_native, PixiScope::All);
}

#[test]
fn build_matrix_only_prunes_to_changed_sizes() {
    // manifest_with_sizes names entries pkg0, pkg1, ... in order.
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4, RunnerSize::Cpu16]);
    let state = MatrixState {
        pixi_native: PixiScope::Only(["pkg0".to_string()].into_iter().collect()),
        ..Default::default()
    };
    let out = build_matrix(&state, &manifest, "RUN");
    // pkg0 is 4cpu only → 1 size × 2 arches = 2 rows, no 16cpu row.
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|e| e.runner_size == "4cpu"));
}
