use super::*;
use std::str::FromStr;
use tempfile::TempDir;

fn recipe(name: &str) -> RecipeName {
    RecipeName::from_str(name).unwrap()
}

fn is_noarch(toml: &str) -> bool {
    UpstreamPixiToml::parse(toml).unwrap().is_noarch()
}

fn test_entry(name: &str, subdir: &str) -> PixiNativeEntry {
    PixiNativeEntry {
        name: name.into(),
        url: GithubRepoUrl::parse("https://github.com/gr/repo").unwrap(),
        rev: Sha40::new("a".repeat(40)).unwrap(),
        subdir: Some(PathBuf::from(subdir)),
        runner_size: RunnerSize::default(),
    }
}

fn index(json: &str) -> ChannelIndex {
    ChannelIndex::from_records(&serde_json::from_str(json).unwrap())
}

const L64: BuildSubdir = BuildSubdir::Arch(Arch::Linux64);
const AARCH: BuildSubdir = BuildSubdir::Arch(Arch::LinuxAarch64);
const NOARCH: BuildSubdir = BuildSubdir::Noarch;

#[test]
fn channel_index_matches_exact_build_and_any_version() {
    let idx = index(
        r#"{"linux-64":[
                 {"name":"autopilot","version":"3.5.4","build_number":0,"subdir":"linux-64"},
                 {"name":"autopilot","version":"3.5.4","build_number":2,"subdir":"linux-64"}
               ]}"#,
    );
    assert!(idx.has_build("autopilot", "3.5.4", 0, L64));
    assert!(idx.has_build("autopilot", "3.5.4", 2, L64));
    // A build we haven't published yet must still read as "needs building".
    assert!(!idx.has_build("autopilot", "3.5.4", 1, L64));
    assert!(!idx.has_build("autopilot", "3.5.5", 0, L64));
    // Dep satisfaction ignores the build number.
    assert!(idx.has_version("autopilot", "3.5.4"));
    assert!(!idx.has_version("autopilot", "3.5.5"));
    assert!(!idx.has_version("geofence", "3.5.4"));
}

#[test]
fn channel_index_sees_noarch_packages() {
    // `pixi search -p linux-64` reports a noarch-only package under the
    // `noarch` key and omits `linux-64` entirely. Ignoring that key made
    // these look unpublished, so every noarch entry rebuilt and republished
    // on every run.
    let idx = index(
        r#"{"noarch":[
                 {"name":"gama_scenarios","version":"1.2.0","build_number":3,"subdir":"noarch"}
               ]}"#,
    );
    assert!(idx.has_build("gama_scenarios", "1.2.0", 3, NOARCH));
    assert!(idx.has_version("gama_scenarios", "1.2.0"));
}

#[test]
fn channel_index_does_not_match_a_build_from_another_subdir() {
    // `vessel_offsets` 1.4.0 really does hold build 1 on linux-64 and build
    // 2 on noarch — a package that moved to noarch mid-version. Matching on
    // name+version+build alone would skip the noarch build 1 we still owe
    // (it only exists on linux-64) and skip the linux-64 build 2 as well.
    let idx = index(
        r#"{"linux-64":[
                 {"name":"vessel_offsets","version":"1.4.0","build_number":1,"subdir":"linux-64"}],
                "noarch":[
                 {"name":"vessel_offsets","version":"1.4.0","build_number":2,"subdir":"noarch"}]}"#,
    );
    assert!(idx.has_build("vessel_offsets", "1.4.0", 1, L64));
    assert!(idx.has_build("vessel_offsets", "1.4.0", 2, NOARCH));
    // The cross-subdir matches that must NOT skip a build.
    assert!(!idx.has_build("vessel_offsets", "1.4.0", 2, L64));
    assert!(!idx.has_build("vessel_offsets", "1.4.0", 1, NOARCH));
    // A sibling arch never satisfies another arch either.
    assert!(!idx.has_build("vessel_offsets", "1.4.0", 1, AARCH));
    // Dep satisfaction is still subdir-agnostic.
    assert!(idx.has_version("vessel_offsets", "1.4.0"));
}

#[test]
fn build_subdir_follows_the_manifest_not_the_job_arch() {
    let noarch = UpstreamPixiToml::parse(
        "[package]\nname=\"p\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-python\"\nversion=\"*\"",
    )
    .unwrap();
    let arch = UpstreamPixiToml::parse("[package]\nname=\"x\"\nversion=\"1\"").unwrap();
    let l64 = TargetPlatform::from_str("linux-64").unwrap();
    let a64 = TargetPlatform::from_str("linux-aarch64").unwrap();

    // A noarch package publishes to `noarch` whichever job builds it.
    assert_eq!(BuildSubdir::of(&noarch, l64), BuildSubdir::Noarch);
    assert_eq!(BuildSubdir::of(&noarch, a64), BuildSubdir::Noarch);
    // Everything else publishes to the job's own arch.
    assert_eq!(
        BuildSubdir::of(&arch, l64),
        BuildSubdir::Arch(Arch::Linux64)
    );
    assert_eq!(
        BuildSubdir::of(&arch, a64),
        BuildSubdir::Arch(Arch::LinuxAarch64)
    );
    // Display must match the subdir keys `pixi search --json` returns.
    assert_eq!(BuildSubdir::Noarch.to_string(), "noarch");
    assert_eq!(
        BuildSubdir::Arch(Arch::LinuxAarch64).to_string(),
        "linux-aarch64"
    );
}

#[test]
fn channel_index_empty_channel_publishes_nothing() {
    // Sweep failure / empty channel must fail open into "needs building",
    // matching what the per-package searches did on error.
    let idx = ChannelIndex::from_records(&BTreeMap::new());
    assert!(!idx.has_build("autopilot", "3.5.4", 0, L64));
    assert!(!idx.has_build("autopilot", "3.5.4", 0, NOARCH));
    assert!(!idx.has_version("autopilot", "3.5.4"));
}

#[test]
fn path_dep_rel_paths_excludes_self_idiom() {
    let t = UpstreamPixiToml::parse(
        "[dependencies]\nnode = { path = \".\" }\n\
             [package]\nname=\"node\"\nversion=\"1.0.0\"\n\
             [package.run-dependencies]\nlib = { path = \"../lib\" }\nros-kilted-rclpy = \"*\"\n\
             [package.host-dependencies]\nmsgs = { path = \"../msgs\" }\n",
    )
    .unwrap();
    let mut got = t.path_dep_rel_paths();
    got.sort();
    assert_eq!(got, vec!["../lib".to_string(), "../msgs".to_string()]);
}

#[test]
fn topo_sort_builds_orders_same_repo_path_deps() {
    let lib = test_entry("lib", "packages/lib");
    let node = test_entry("node", "packages/node");
    let items = vec![
        BuildItem {
            entry: &node,
            effective_build: 0,
            name: "node".into(),
            rel_path_deps: vec!["../lib".into()],
            pin_dep_names: vec![],
        },
        BuildItem {
            entry: &lib,
            effective_build: 0,
            name: "lib".into(),
            rel_path_deps: vec![],
            pin_dep_names: vec![],
        },
    ];
    let sorted = topo_sort_builds(items).unwrap();
    assert_eq!(sorted[0].name, "lib");
    assert_eq!(sorted[1].name, "node");
}

#[test]
fn topo_sort_builds_orders_same_repo_pin_deps() {
    // node pins lib by its channel artifact name (entry.name), not a path.
    let lib = test_entry("lib", "packages/lib");
    let node = test_entry("node", "packages/node");
    let items = vec![
        BuildItem {
            entry: &node,
            effective_build: 0,
            name: "node".into(),
            rel_path_deps: vec![],
            pin_dep_names: vec!["lib".into()],
        },
        BuildItem {
            entry: &lib,
            effective_build: 0,
            name: "lib".into(),
            rel_path_deps: vec![],
            pin_dep_names: vec![],
        },
    ];
    let sorted = topo_sort_builds(items).unwrap();
    assert_eq!(sorted[0].name, "lib");
    assert_eq!(sorted[1].name, "node");
}

#[test]
fn topo_sort_builds_rejects_cycles() {
    let a = test_entry("a", "packages/a");
    let b = test_entry("b", "packages/b");
    let items = vec![
        BuildItem {
            entry: &a,
            effective_build: 0,
            name: "a".into(),
            rel_path_deps: vec!["../b".into()],
            pin_dep_names: vec![],
        },
        BuildItem {
            entry: &b,
            effective_build: 0,
            name: "b".into(),
            rel_path_deps: vec!["../a".into()],
            pin_dep_names: vec![],
        },
    ];
    assert!(
        topo_sort_builds(items)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn noarch_detects_python_and_ament_python() {
    // pixi-build-python backend → noarch
    assert!(is_noarch(
        "[package]\nname=\"c\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-python\"\nversion=\"*\"",
    ));
    // ament_python ROS package → noarch
    assert!(is_noarch(
        "[package]\nname=\"p\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-ros-gr\"\n\
             [package.build.config]\nbuild-type=\"ament_python\"",
    ));
}

#[test]
fn noarch_false_for_compiled_and_missing_build() {
    // ament_cmake / ament_idl compile per-arch → not noarch
    for bt in ["ament_cmake", "ament_idl", "cmake"] {
        assert!(
            !is_noarch(&format!(
                "[package]\nname=\"x\"\nversion=\"1\"\n\
                     [package.build.backend]\nname=\"pixi-build-ros-gr\"\n\
                     [package.build.config]\nbuild-type=\"{bt}\"",
            )),
            "expected {bt} to be arch-specific"
        );
    }
    // no [package.build] at all → conservative: not noarch
    assert!(!is_noarch("[package]\nname=\"x\"\nversion=\"1\""));
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

#[test]
fn upstream_pixi_toml_parses_minimal() {
    let text = r#"
[package]
name = "foo"
version = "1.2.3"
"#;
    let p = UpstreamPixiToml::parse(text).unwrap();
    assert_eq!(p.package.name, "foo");
    assert_eq!(p.package.version, "1.2.3");
    assert_eq!(p.build_number(), 0);
}

#[test]
fn upstream_pixi_toml_parses_build_number() {
    let text = r#"
[package]
name = "foo"
version = "1.2.3"

[package.build.config]
build-number = 5
"#;
    let p = UpstreamPixiToml::parse(text).unwrap();
    assert_eq!(p.build_number(), 5);
}

#[test]
fn upstream_pixi_toml_supports_platform_when_empty() {
    let text = r#"
[package]
name = "foo"
version = "1.0"
"#;
    let p = UpstreamPixiToml::parse(text).unwrap();
    assert!(p.supports_platform(TargetPlatform::default()));
}

#[test]
fn upstream_pixi_toml_respects_platforms_list() {
    let text = r#"
[package]
name = "foo"
version = "1.0"

[workspace]
platforms = ["linux-64"]
"#;
    let p = UpstreamPixiToml::parse(text).unwrap();
    assert!(p.supports_platform(TargetPlatform::default()));
    let aarch = TargetPlatform::from_str("linux-aarch64").unwrap();
    assert!(!p.supports_platform(aarch));
}

#[test]
fn upstream_pixi_toml_ignores_unknown_keys() {
    let text = r#"
[package]
name = "foo"
version = "1.0"

[tasks]
ci = "test"

[dependencies]
something = "1"
"#;
    UpstreamPixiToml::parse(text).unwrap();
}

fn write_tmp_pixi_toml(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pixi.toml");
    std::fs::write(&path, text).unwrap();
    (tmp, path)
}

#[test]
fn rewrite_build_number_updates_existing_field() {
    let original = r#"[package]
name = "foo"
version = "1.0"

[package.build.config]
build-number = 0
"#;
    let (_tmp, path) = write_tmp_pixi_toml(original);
    rewrite_build_number(&path, 5).unwrap();
    let updated = std::fs::read_to_string(&path).unwrap();
    let reparsed = UpstreamPixiToml::parse(&updated).unwrap();
    assert_eq!(reparsed.build_number(), 5);
}

#[test]
fn rewrite_build_number_inserts_when_absent() {
    let original = r#"[package]
name = "foo"
version = "1.0"
"#;
    let (_tmp, path) = write_tmp_pixi_toml(original);
    rewrite_build_number(&path, 2).unwrap();
    let updated = std::fs::read_to_string(&path).unwrap();
    let reparsed = UpstreamPixiToml::parse(&updated).unwrap();
    assert_eq!(reparsed.build_number(), 2);
}

#[test]
fn rewrite_build_number_is_idempotent() {
    let original = r#"[package]
name = "foo"
version = "1.0"

[package.build.config]
build-number = 0
"#;
    let (_tmp, path) = write_tmp_pixi_toml(original);
    rewrite_build_number(&path, 4).unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    rewrite_build_number(&path, 4).unwrap();
    let second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(first, second);
}

#[test]
fn rewrite_build_number_preserves_unrelated_keys_and_comments() {
    let original = r#"# top-of-file comment
[package]
name = "foo"  # inline comment
version = "1.0"

[tasks]
ci = "test"
"#;
    let (_tmp, path) = write_tmp_pixi_toml(original);
    rewrite_build_number(&path, 1).unwrap();
    let updated = std::fs::read_to_string(&path).unwrap();
    assert!(updated.contains("# top-of-file comment"), "got: {updated}");
    assert!(updated.contains("# inline comment"), "got: {updated}");
    assert!(updated.contains("ci = \"test\""), "got: {updated}");
    assert!(updated.contains("build-number = 1"), "got: {updated}");
}

#[test]
fn rewrite_build_number_errors_when_package_missing() {
    let original = "[tasks]\nci = \"test\"\n";
    let (_tmp, path) = write_tmp_pixi_toml(original);
    let err = rewrite_build_number(&path, 1).unwrap_err();
    assert!(format!("{err:#}").contains("missing [package]"));
}

fn write_checkout_pkg(root: &Path, name: &str, extra: &str) -> PathBuf {
    let dir = root.join("packages").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("pixi.toml");
    std::fs::write(
        &p,
        format!(
            "[workspace]\nname = \"{name}\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
                 [dependencies]\n{name} = {{ path = \".\" }}\n\
                 [package]\nname = \"{name}\"\nversion = \"2.5.0\"\n{extra}"
        ),
    )
    .unwrap();
    p
}

#[test]
fn resolve_path_deps_rewrites_to_sibling_manifest_version() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", "");
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = { path = \"../lib\" }\nros-kilted-rclpy = \"*\"\n",
    );
    let resolved = resolve_path_deps(&consumer).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "lib");
    assert_eq!(resolved[0].version, "2.5.0");

    let text = std::fs::read_to_string(&consumer).unwrap();
    assert!(text.contains("lib = \">=2.5.0,<3\""), "rewritten: {text}");
    assert!(
        text.contains("node = { path = \".\" }"),
        "self idiom untouched: {text}"
    );
    assert!(
        text.contains("ros-kilted-rclpy"),
        "externals untouched: {text}"
    );
}

#[test]
fn resolve_path_deps_uses_dep_key_not_sibling_package_name() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    // Sibling dir/manifest is named "lib" (e.g. package-xml mode: no
    // stable relationship between package.name and the channel artifact
    // name), but the consumer depends on it under the artifact-name key
    // "ros-kilted-lib".
    write_checkout_pkg(root, "lib", "");
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nros-kilted-lib = { path = \"../lib\" }\n",
    );
    let resolved = resolve_path_deps(&consumer).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "ros-kilted-lib");
    assert_eq!(resolved[0].version, "2.5.0");

    let text = std::fs::read_to_string(&consumer).unwrap();
    assert!(
        text.contains("ros-kilted-lib = \">=2.5.0,<3\""),
        "rewritten under the dep key: {text}"
    );
}

#[test]
fn resolve_path_deps_errors_clearly_when_sibling_has_no_version() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let dir = root.join("packages").join("lib");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("pixi.toml"),
        "[workspace]\nname = \"lib\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
             [dependencies]\nlib = { path = \".\" }\n\
             [package]\nname = \"lib\"\n",
    )
    .unwrap();
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = { path = \"../lib\" }\n",
    );
    let err = resolve_path_deps(&consumer).unwrap_err();
    assert!(
        format!("{err:#}").contains("package.version"),
        "got: {err:#}"
    );
}

#[test]
fn range_pin_derives_major_cap() {
    assert_eq!(range_pin("2.5.0").unwrap(), ">=2.5.0,<3");
    assert_eq!(range_pin("1.24.0-alpha.2").unwrap(), ">=1.24.0-alpha.2,<2");
    assert_eq!(range_pin("0.3.1").unwrap(), ">=0.3.1,<1");
}

#[test]
fn range_pin_rejects_non_numeric_major() {
    let err = range_pin("rolling").unwrap_err();
    assert!(format!("{err:#}").contains("numeric major"), "got: {err:#}");
}

#[test]
fn resolved_dep_version_stays_the_bare_floor() {
    // Fallback/availability machinery keys on the exact floor version, not
    // the range — a regression here would break cross-bucket builds.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", "");
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = { path = \"../lib\" }\n",
    );
    let resolved = resolve_path_deps(&consumer).unwrap();
    assert_eq!(resolved[0].version, "2.5.0");
}

#[test]
fn exact_pin_version_parses_only_concrete_triples() {
    assert_eq!(exact_pin_version("==2.5.0"), Some("2.5.0"));
    assert_eq!(exact_pin_version("==2.5.0-alpha.1"), Some("2.5.0-alpha.1"));
    assert_eq!(exact_pin_version(">=2.5.0"), None);
    assert_eq!(exact_pin_version("==2.5.*"), None);
    assert_eq!(exact_pin_version("*"), None);
    assert_eq!(exact_pin_version("==2.5"), None); // not three components
}

#[test]
fn exact_pins_lists_pin_keys_across_tables() {
    let t = UpstreamPixiToml::parse(
        "[package]\nname=\"node\"\nversion=\"1.0.0\"\n\
             [package.run-dependencies]\nlib = \"==2.5.0\"\nros-kilted-rclpy = \"*\"\n\
             [package.host-dependencies]\nmsgs = \"==1.2.3\"\n",
    )
    .unwrap();
    let mut got: Vec<String> = t.exact_pins().into_iter().map(|(k, _)| k).collect();
    got.sort();
    assert_eq!(got, vec!["lib".to_string(), "msgs".to_string()]);
}

/// dep-name -> subdir map for `resolve_sibling_pins`, matching how the main
/// loop derives it from same-repo `manifest.packages` entries.
fn subdirs(pairs: &[(&str, &str)]) -> BTreeMap<String, PathBuf> {
    pairs
        .iter()
        .map(|(n, d)| (n.to_string(), PathBuf::from(d)))
        .collect()
}

#[test]
fn resolve_sibling_pins_resolves_matching_pin() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", ""); // version 2.5.0
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = \"==2.5.0\"\n",
    );
    let map = subdirs(&[("lib", "packages/lib")]);
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "lib");
    assert_eq!(resolved[0].version, "2.5.0");
    assert_eq!(resolved[0].manifest, root.join("packages/lib/pixi.toml"));
}

#[test]
fn resolve_sibling_pins_skips_version_mismatch() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", ""); // checkout is 2.5.0
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = \"==2.4.0\"\n", // older pin
    );
    let map = subdirs(&[("lib", "packages/lib")]);
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert!(resolved.is_empty(), "older pin left to the real channel");
}

#[test]
fn resolve_sibling_pins_ignores_non_sibling_pins() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nros-kilted-rclpy = \"==1.0.0\"\n",
    );
    let map = subdirs(&[("lib", "packages/lib")]); // rclpy not in map
    let resolved = resolve_sibling_pins(&consumer, root, &map).unwrap();
    assert!(resolved.is_empty());
}

#[test]
fn check_local_build_guard_detects_cycle() {
    let visiting = vec!["a".to_string(), "b".to_string()];
    let local_built = BTreeSet::new();
    let err = check_local_build_guard("a", &visiting, &local_built).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cycle"), "message: {msg}");
    assert!(msg.contains("a -> b -> a"), "message: {msg}");
}

#[test]
fn check_local_build_guard_skips_already_built() {
    let visiting: Vec<String> = Vec::new();
    let mut local_built = BTreeSet::new();
    local_built.insert("lib".to_string());
    assert!(!check_local_build_guard("lib", &visiting, &local_built).unwrap());
    // Not yet built and not visiting: proceed.
    assert!(check_local_build_guard("other", &visiting, &local_built).unwrap());
}

#[test]
fn prepend_channels_front_inserts() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("pixi.toml");
    std::fs::write(
        &p,
        "[workspace]\nname = \"x\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
             [package]\nname = \"x\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    prepend_channels(&p, &["file:///out".into(), "file:///local-deps".into()]).unwrap();
    let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let ch: Vec<&str> = doc["workspace"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        ch,
        vec![
            "file:///out",
            "file:///local-deps",
            "https://prefix.dev/conda-forge"
        ]
    );
}

#[test]
fn select_entries_filters_by_only_and_size() {
    let yaml = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
    runner-size: 4cpu
  - name: beta
    url: https://github.com/org/beta
    rev: 2222222222222222222222222222222222222222
    runner-size: 8cpu
  - name: gamma
    url: https://github.com/org/gamma
    rev: 3333333333333333333333333333333333333333
    runner-size: 4cpu
"#;
    let m = crate::types::PixiNativeManifest::from_yaml_str(yaml).unwrap();

    // --only alpha,beta with no size filter → alpha, beta
    let sel = select_entries(&m.packages, None, &["alpha".into(), "beta".into()]);
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);

    // --only alpha,beta + runner-size 4cpu → alpha only
    let sel = select_entries(
        &m.packages,
        Some(crate::types::RunnerSize::Cpu4),
        &["alpha".into(), "beta".into()],
    );
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha"]);

    // empty --only → size filter only (all 4cpu)
    let sel = select_entries(&m.packages, Some(crate::types::RunnerSize::Cpu4), &[]);
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "gamma"]);
}
