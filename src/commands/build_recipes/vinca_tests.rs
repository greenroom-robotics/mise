use super::*;
use std::str::FromStr;
use tempfile::TempDir;

fn recipe(name: &str) -> RecipeName {
    RecipeName::from_str(name).unwrap()
}

#[test]
fn vinca_mode_normal_when_no_flags() {
    let m = VincaBuildMode::from_flags(vec![], None, vec![]).unwrap();
    assert_eq!(m, VincaBuildMode::Normal);
}

#[test]
fn vinca_mode_drop_when_only_recipes() {
    let m = VincaBuildMode::from_flags(vec![recipe("a"), recipe("b")], None, vec![]).unwrap();
    assert_eq!(
        m,
        VincaBuildMode::DropDeepstream {
            recipes: vec![recipe("a"), recipe("b")]
        }
    );
}

#[test]
fn vinca_mode_deepstream_only_when_ds_recipe_and_version() {
    let m = VincaBuildMode::from_flags(vec![recipe("a")], Some(DeepstreamVersion::V7_1), vec![])
        .unwrap();
    assert_eq!(
        m,
        VincaBuildMode::DeepstreamOnly {
            recipes: vec![recipe("a")],
            version: DeepstreamVersion::V7_1,
        }
    );
}

#[test]
fn vinca_mode_rejects_version_without_recipes() {
    let err =
        VincaBuildMode::from_flags(vec![], Some(DeepstreamVersion::V8_0), vec![]).unwrap_err();
    assert!(format!("{err:#}").contains("requires at least one --ds-recipe or --only"));
}

#[test]
fn vinca_mode_only_alone_unpinned() {
    let m = VincaBuildMode::from_flags(vec![], None, vec![recipe("foo")]).unwrap();
    assert_eq!(
        m,
        VincaBuildMode::Only {
            recipes: vec![recipe("foo")],
            version: None,
        }
    );
}

#[test]
fn vinca_mode_only_with_ds_version_pins() {
    let m = VincaBuildMode::from_flags(vec![], Some(DeepstreamVersion::V8_0), vec![recipe("foo")])
        .unwrap();
    assert_eq!(
        m,
        VincaBuildMode::Only {
            recipes: vec![recipe("foo")],
            version: Some(DeepstreamVersion::V8_0),
        }
    );
}

#[test]
fn vinca_mode_only_rejects_combined_with_ds_recipe() {
    let err = VincaBuildMode::from_flags(vec![recipe("a")], None, vec![recipe("b")]).unwrap_err();
    assert!(format!("{err:#}").contains("mutually exclusive"));
}

fn write_recipe_dir(parent: &Path, name: &str) {
    let d = parent.join(name);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("recipe.yaml"), "marker").unwrap();
}

fn make_repo_with_recipes(recipe_names: &[&str], vendor_names: &[&str]) -> TempDir {
    let td = TempDir::new().unwrap();
    let recipes = td.path().join("recipes");
    fs::create_dir_all(&recipes).unwrap();
    for n in recipe_names {
        write_recipe_dir(&recipes, n);
    }
    if !vendor_names.is_empty() {
        let vendor = td.path().join("vendor_recipes");
        fs::create_dir_all(&vendor).unwrap();
        for n in vendor_names {
            write_recipe_dir(&vendor, n);
        }
    }
    td
}

fn recipe_names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().unwrap().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn apply_filter_overlays_vendor_recipes() {
    let td = make_repo_with_recipes(&["foo"], &["bar"]);
    apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
    assert_eq!(
        recipe_names_in(&td.path().join("recipes")),
        vec!["bar", "foo"]
    );
}

#[test]
fn apply_filter_overlays_vendor_overwriting() {
    // recipes/foo exists with content "marker"; vendor_recipes/foo exists too
    let td = TempDir::new().unwrap();
    let recipes = td.path().join("recipes");
    fs::create_dir_all(&recipes).unwrap();
    write_recipe_dir(&recipes, "foo");
    fs::write(recipes.join("foo/old.txt"), "old").unwrap();
    let vendor = td.path().join("vendor_recipes");
    fs::create_dir_all(&vendor).unwrap();
    write_recipe_dir(&vendor, "foo");
    fs::write(vendor.join("foo/new.txt"), "new").unwrap();

    apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
    // After overlay, recipes/foo/new.txt should exist and recipes/foo/old.txt should not.
    assert!(recipes.join("foo/new.txt").exists());
    assert!(!recipes.join("foo/old.txt").exists());
}

#[test]
fn apply_filter_removes_deepstream_mutex() {
    let td = make_repo_with_recipes(&["foo", "deepstream-mutex"], &[]);
    apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
    assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
}

#[test]
fn apply_filter_drop_deepstream_removes_listed() {
    let td = make_repo_with_recipes(&["foo", "deepstream-a", "deepstream-b"], &[]);
    apply_recipe_filter(
        td.path(),
        &VincaBuildMode::DropDeepstream {
            recipes: vec![recipe("deepstream-a"), recipe("deepstream-b")],
        },
    )
    .unwrap();
    assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
}

#[test]
fn apply_filter_deepstream_only_keeps_listed() {
    let td = make_repo_with_recipes(&["foo", "deepstream-a", "deepstream-b"], &[]);
    apply_recipe_filter(
        td.path(),
        &VincaBuildMode::DeepstreamOnly {
            recipes: vec![recipe("deepstream-a")],
            version: DeepstreamVersion::V7_1,
        },
    )
    .unwrap();
    assert_eq!(
        recipe_names_in(&td.path().join("recipes")),
        vec!["deepstream-a"]
    );
}

#[test]
fn apply_filter_only_keeps_listed_regardless_of_ds() {
    let td = make_repo_with_recipes(&["foo", "bar", "deepstream-a"], &[]);
    apply_recipe_filter(
        td.path(),
        &VincaBuildMode::Only {
            recipes: vec![recipe("foo")],
            version: None,
        },
    )
    .unwrap();
    assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
}

#[test]
fn write_variants_pin_v71_pins_gcc_13() {
    let tf = write_variants_pin(DeepstreamVersion::V7_1).unwrap();
    let content = fs::read_to_string(tf.path()).unwrap();
    assert!(
        content.contains("deepstream_version:\n  - \"7.1\""),
        "got: {content}"
    );
    assert!(
        content.contains("c_compiler_version:\n  - \"13\""),
        "got: {content}"
    );
    assert!(
        content.contains("cxx_compiler_version:\n  - \"13\""),
        "got: {content}"
    );
}

#[test]
fn write_variants_pin_v80_no_compiler_pin() {
    let tf = write_variants_pin(DeepstreamVersion::V8_0).unwrap();
    let content = fs::read_to_string(tf.path()).unwrap();
    assert!(
        content.contains("deepstream_version:\n  - \"8.0\""),
        "got: {content}"
    );
    assert!(
        !content.contains("c_compiler_version"),
        "should not pin gcc for 8.0: {content}"
    );
}

#[test]
fn version_extracts_from_deepstream_only() {
    assert_eq!(VincaBuildMode::Normal.version(), None);
    assert_eq!(
        VincaBuildMode::DropDeepstream {
            recipes: vec![recipe("a")]
        }
        .version(),
        None,
    );
    assert_eq!(
        VincaBuildMode::DeepstreamOnly {
            recipes: vec![recipe("a")],
            version: DeepstreamVersion::V8_0,
        }
        .version(),
        Some(DeepstreamVersion::V8_0),
    );
    assert_eq!(
        VincaBuildMode::Only {
            recipes: vec![recipe("a")],
            version: None,
        }
        .version(),
        None,
    );
    assert_eq!(
        VincaBuildMode::Only {
            recipes: vec![recipe("a")],
            version: Some(DeepstreamVersion::V7_1),
        }
        .version(),
        Some(DeepstreamVersion::V7_1),
    );
}
