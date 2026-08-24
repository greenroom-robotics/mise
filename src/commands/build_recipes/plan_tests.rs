use super::*;
use std::path::PathBuf;

use crate::types::{GithubRepoUrl, RunnerSize, Sha40, SiblingPinStyle};

fn pkg(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

fn test_entry(name: &str, subdir: &str) -> PixiNativeEntry {
    PixiNativeEntry {
        name: pkg(name),
        url: GithubRepoUrl::parse("https://github.com/gr/repo").unwrap(),
        rev: Sha40::new("a".repeat(40)).unwrap(),
        subdir: Some(PathBuf::from(subdir)),
        runner_size: RunnerSize::default(),
        himem: false,
        lfs: false,
        pin_style: SiblingPinStyle::Range,
    }
}

#[test]
fn build_plan_orders_same_repo_path_deps() {
    let lib = test_entry("lib", "packages/lib");
    let node = test_entry("node", "packages/node");
    let items = vec![
        BuildItem {
            entry: &node,
            effective_build: 0,
            name: pkg("node"),
            rel_path_deps: vec!["../lib".into()],
            pin_dep_names: vec![],
        },
        BuildItem {
            entry: &lib,
            effective_build: 0,
            name: pkg("lib"),
            rel_path_deps: vec![],
            pin_dep_names: vec![],
        },
    ];
    let sorted: Vec<BuildItem> = BuildPlan::new(items).unwrap().into_iter().collect();
    assert_eq!(sorted[0].name.as_str(), "lib");
    assert_eq!(sorted[1].name.as_str(), "node");
}

#[test]
fn build_plan_orders_same_repo_pin_deps() {
    // node pins lib by its channel artifact name (entry.name), not a path.
    let lib = test_entry("lib", "packages/lib");
    let node = test_entry("node", "packages/node");
    let items = vec![
        BuildItem {
            entry: &node,
            effective_build: 0,
            name: pkg("node"),
            rel_path_deps: vec![],
            pin_dep_names: vec![pkg("lib")],
        },
        BuildItem {
            entry: &lib,
            effective_build: 0,
            name: pkg("lib"),
            rel_path_deps: vec![],
            pin_dep_names: vec![],
        },
    ];
    let sorted: Vec<BuildItem> = BuildPlan::new(items).unwrap().into_iter().collect();
    assert_eq!(sorted[0].name.as_str(), "lib");
    assert_eq!(sorted[1].name.as_str(), "node");
}

#[test]
fn build_plan_rejects_cycles() {
    let a = test_entry("a", "packages/a");
    let b = test_entry("b", "packages/b");
    let items = vec![
        BuildItem {
            entry: &a,
            effective_build: 0,
            name: pkg("a"),
            rel_path_deps: vec!["../b".into()],
            pin_dep_names: vec![],
        },
        BuildItem {
            entry: &b,
            effective_build: 0,
            name: pkg("b"),
            rel_path_deps: vec!["../a".into()],
            pin_dep_names: vec![],
        },
    ];
    assert!(
        BuildPlan::new(items)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}
