use super::*;
use crate::types::PackageName;

fn pkg(s: &str) -> PackageName {
    PackageName::new(s).unwrap()
}

/// The runner size of a row that must be a pixi-native one.
fn runner_size(row: &MatrixRow) -> RunnerSize {
    match row.kind {
        RowKind::PixiNative { runner } => runner,
        ref other => panic!("expected a pixi-native row, got {other:?}"),
    }
}

/// The DS version of a row that must be a DeepStream one.
fn ds_version(row: &MatrixRow) -> DeepstreamVersion {
    match row.kind {
        RowKind::Deepstream { version } => version,
        ref other => panic!("expected a DeepStream row, got {other:?}"),
    }
}

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
fn matrix_row_serializes_with_kebab_case_keys() {
    let row = MatrixRow::vinca(Arch::Linux64, "runs-on={run_id}/runner=8cpu-linux-x64", "1");
    let json = serde_json::to_string(&row).unwrap();
    assert_eq!(
        json,
        r#"{"pipeline":"vinca","target-platform":"linux-64","ds-version":"","ds-image":"","runner":"runs-on=1/runner=8cpu-linux-x64","runner-size":"","artifact-name":"build-linux-64"}"#
    );
}

#[test]
fn pixi_native_row_fills_runner_size_and_leaves_ds_empty() {
    let row = MatrixRow::pixi_native(Arch::LinuxAarch64, RunnerSize::Cpu16, "7");
    let json = serde_json::to_string(&row).unwrap();
    assert_eq!(
        json,
        r#"{"pipeline":"pixi-native","target-platform":"linux-aarch64","ds-version":"","ds-image":"","runner":"runs-on=7/runner=16cpu-linux-arm64","runner-size":"16cpu","artifact-name":"build-pixi-native-linux-aarch64-16cpu"}"#
    );
}

#[test]
fn deepstream_row_runs_the_vinca_pipeline_and_derives_its_image() {
    let row = MatrixRow::deepstream(Arch::Linux64, DeepstreamVersion::V7_1, "7");
    let json = serde_json::to_string(&row).unwrap();
    assert_eq!(
        json,
        r#"{"pipeline":"vinca","target-platform":"linux-64","ds-version":"7.1","ds-image":"nvcr.io/nvidia/deepstream:7.1-triton-multiarch","runner":"runs-on=7/family=c6id.xlarge/image=deepstream-x64-7.1","runner-size":"","artifact-name":"build-deepstream-linux-64-ds7.1"}"#
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
    assert_eq!(s.pixi_native, RawScope::All);
    assert_eq!(s.ds_versions.len(), 2);
}

#[test]
fn classify_global_vinca_triggers_vinca_and_ds() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vinca.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
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
    assert_eq!(s.pixi_native, RawScope::All);
    assert_eq!(s.ds_versions.len(), 2);
}

#[test]
fn classify_workflow_prefix_triggers_everything() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&[".github/workflows/build.yml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, RawScope::All);
    assert!(!s.ds_versions.is_empty());
}

#[test]
fn classify_deepstream_recipes_file_triggers_vinca_and_ds() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&[".github/deepstream-recipes.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V7_1));
}

#[test]
fn classify_variants_file_triggers_only_ds() {
    let ds = cfg(&[], &[DeepstreamVersion::V8_0]);
    let s = classify(&paths(&["variants/deepstream.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V8_0));
}

#[test]
fn classify_rosdistro_additional_triggers_vinca_only() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["rosdistro_additional_recipes.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_pixi_native_manifest_triggers_pixi_only() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi_native_packages.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, RawScope::ManifestScoped);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_vendor_recipes_ds_match_triggers_ds_only() {
    let ds = cfg(&["my-ds-recipe"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vendor_recipes/my-ds-recipe/recipe.yaml"]), &ds);
    assert!(!s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
    assert!(s.ds_versions.contains(&DeepstreamVersion::V7_1));
}

#[test]
fn classify_vendor_recipes_non_ds_triggers_vinca() {
    let ds = cfg(&["other-recipe"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["vendor_recipes/regular-recipe/recipe.yaml"]), &ds);
    assert!(s.vinca);
    assert_eq!(s.pixi_native, RawScope::None);
    assert!(s.ds_versions.is_empty());
}

#[test]
fn classify_recipes_path_is_ignored() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["recipes/some-generated/recipe.yaml"]), &ds);
    assert_eq!(s, RawState::default());
}

#[test]
fn classify_unrelated_paths_yield_no_work() {
    let ds = cfg(&[], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["README.md", "docs/some-doc.md"]), &ds);
    assert_eq!(s, RawState::default());
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
            name: PackageName::new(format!("pkg{i}")).unwrap(),
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
fn build_matrix_empty_plan_yields_nothing() {
    let plan = MatrixPlan::default();
    let out = build_matrix(&plan, &empty_manifest(), "RUN");
    assert!(out.is_empty());
}

#[test]
fn build_matrix_vinca_only_produces_two_arches() {
    let plan = MatrixPlan {
        vinca: true,
        ..Default::default()
    };
    let out = build_matrix(&plan, &empty_manifest(), "RUN");
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|e| e.kind == RowKind::Vinca));
    assert!(out.iter().any(|e| e.target_platform == Arch::Linux64));
    assert!(out.iter().any(|e| e.target_platform == Arch::LinuxAarch64));
    assert!(out[0].runner.contains("RUN"));
}

#[test]
fn build_matrix_pixi_native_groups_by_size() {
    let plan = MatrixPlan {
        pixi_native: PixiScope::All,
        ..Default::default()
    };
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4, RunnerSize::Cpu4, RunnerSize::Cpu8]);
    let out = build_matrix(&plan, &manifest, "RUN");
    // Two unique sizes (4, 8) × 2 arches = 4 rows
    assert_eq!(out.len(), 4);
    let mut sizes: Vec<RunnerSize> = out.iter().map(runner_size).collect();
    sizes.sort();
    sizes.dedup();
    assert_eq!(sizes, vec![RunnerSize::Cpu4, RunnerSize::Cpu8]);
}

#[test]
fn build_matrix_pixi_native_uses_correct_arch_tag() {
    let plan = MatrixPlan {
        pixi_native: PixiScope::All,
        ..Default::default()
    };
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4]);
    let out = build_matrix(&plan, &manifest, "RUN");
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
    let mut plan = MatrixPlan::default();
    plan.ds_versions.insert(DeepstreamVersion::V7_1);
    plan.ds_versions.insert(DeepstreamVersion::V8_0);
    let out = build_matrix(&plan, &empty_manifest(), "RUN");
    assert_eq!(out.len(), 4);
    let v71_x64 = out
        .iter()
        .find(|e| {
            e.kind
                == RowKind::Deepstream {
                    version: DeepstreamVersion::V7_1,
                }
                && e.target_platform == Arch::Linux64
        })
        .unwrap();
    assert!(v71_x64.runner.contains("family=c6id.xlarge"));
    assert!(v71_x64.runner.contains("deepstream-x64-7.1"));
    let v80_arm = out
        .iter()
        .find(|e| {
            e.kind
                == RowKind::Deepstream {
                    version: DeepstreamVersion::V8_0,
                }
                && e.target_platform == Arch::LinuxAarch64
        })
        .unwrap();
    assert!(v80_arm.runner.contains("family=c7gd.xlarge"));
    assert!(v80_arm.runner.contains("deepstream-arm64-8.0"));
}

#[test]
fn build_matrix_ds_versions_sorted_ascending() {
    let mut plan = MatrixPlan::default();
    plan.ds_versions.insert(DeepstreamVersion::V8_0);
    plan.ds_versions.insert(DeepstreamVersion::V7_1);
    let out = build_matrix(&plan, &empty_manifest(), "RUN");
    // First 2 rows should be 7.1, last 2 should be 8.0
    let versions: Vec<DeepstreamVersion> = out.iter().map(ds_version).collect();
    use DeepstreamVersion::{V7_1, V8_0};
    assert_eq!(versions, vec![V7_1, V7_1, V8_0, V8_0]);
}

#[test]
fn should_not_run_row_is_the_sentinel() {
    let row = MatrixRow::should_not_run("RUN");
    assert_eq!(row.kind, RowKind::ShouldNotRun);
    assert_eq!(row.kind.pipeline(), Pipeline::ShouldNotRun);
    assert_eq!(row.artifact_name, "should-not-run");
    assert!(row.runner.contains("RUN"));
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
    assert_eq!(changed, ["added", "beta"].into_iter().map(pkg).collect());
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
    assert_eq!(changed, ["alpha"].into_iter().map(pkg).collect());
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
    assert_eq!(changed, ["alpha", "beta"].into_iter().map(pkg).collect());
}

#[test]
fn classify_manifest_only_is_scoped() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi_native_packages.yaml"]), &ds);
    assert_eq!(s.pixi_native, RawScope::ManifestScoped);
    assert!(!s.vinca);
}

#[test]
fn classify_global_both_forces_pixi_all() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&paths(&["pixi.toml", "pixi_native_packages.yaml"]), &ds);
    assert_eq!(s.pixi_native, RawScope::All);
}

#[test]
fn classify_changedfiles_all_is_pixi_all() {
    let ds = cfg(&["foo"], &[DeepstreamVersion::V7_1]);
    let s = classify(&ChangedFiles::All, &ds);
    assert_eq!(s.pixi_native, RawScope::All);
}

#[test]
fn build_matrix_only_prunes_to_changed_sizes() {
    // manifest_with_sizes names entries pkg0, pkg1, ... in order.
    let manifest = manifest_with_sizes(&[RunnerSize::Cpu4, RunnerSize::Cpu16]);
    let plan = MatrixPlan {
        pixi_native: PixiScope::Only(["pkg0"].into_iter().map(pkg).collect()),
        ..Default::default()
    };
    let out = build_matrix(&plan, &manifest, "RUN");
    // pkg0 is 4cpu only → 1 size × 2 arches = 2 rows, no 16cpu row.
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|e| runner_size(e) == RunnerSize::Cpu4));
}
