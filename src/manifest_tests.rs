use super::*;
use crate::types::{Arch, ChannelUrl, PackageName, SiblingPinStyle, Version};

fn pn(s: &str) -> PackageName {
    PackageName::new(s).unwrap()
}

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}
use std::fs;
use tempfile::TempDir;

fn make_pkg(root: &Path, name: &str) {
    let pkg = root.join(name);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        format!(
            "[workspace]\nname = \"{name}\"\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"
        ),
    )
    .unwrap();
}

/// A dev-environment manifest: workspace, no `[package]`.
fn make_workspace_only(root: &Path, name: &str) {
    let pkg = root.join(name);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        format!("[workspace]\nname = \"{name}\"\n[tasks]\nbuild = \"colcon build\"\n"),
    )
    .unwrap();
}

fn manifest_paths(pkgs: &[Package]) -> Vec<PathBuf> {
    pkgs.iter().map(|p| p.manifest_path.clone()).collect()
}

// ---------------------------------------------------------------------------
// Read view
// ---------------------------------------------------------------------------

#[test]
fn reads_name_and_version() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("pixi.toml");
    fs::write(
        &p,
        "[workspace]\nname = \"foo\"\n[package]\nname = \"foo\"\nversion = \"1.2.3\"\n",
    )
    .unwrap();
    let pkg = Package::read(&p).unwrap();
    assert_eq!(
        pkg.identity(),
        PackageIdentity {
            name: pn("foo"),
            version: ver("1.2.3")
        }
    );
    assert_eq!(pkg.dir, tmp.path());
}

#[test]
fn errors_when_package_section_missing() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("pixi.toml");
    fs::write(&p, "[workspace]\nname = \"foo\"\n").unwrap();
    let err = Package::read(&p).unwrap_err();
    assert!(err.to_string().contains("no [package] section"));
}

#[test]
fn workspace_only_manifest_parses_as_its_own_variant() {
    assert!(matches!(
        Manifest::parse("[workspace]\nname = \"foo\"\n").unwrap(),
        Manifest::WorkspaceOnly
    ));
}

// Identity is mandatory: a [package] table missing either key is not a
// package this tool can release, and saying so at the manifest is what makes
// `identity()` infallible everywhere downstream.
#[test]
fn package_without_a_name_is_rejected_at_parse() {
    let err = PackageManifest::parse("[package]\nversion = \"1.0.0\"\n").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("[package]"), "got: {msg}");
    assert!(msg.contains("missing field `name`"), "got: {msg}");
}

#[test]
fn package_without_a_version_is_rejected_at_parse() {
    let err = PackageManifest::parse("[package]\nname = \"foo\"\n").unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("[package]"), "got: {msg}");
    assert!(msg.contains("missing field `version`"), "got: {msg}");
}

// ...and the file is named, so the report points at the manifest to fix.
#[test]
fn reading_a_manifest_with_no_package_name_names_the_file() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("pixi.toml");
    fs::write(&p, "[package]\nversion = \"1.0.0\"\n").unwrap();
    let msg = format!("{:#}", Package::read(&p).unwrap_err());
    assert!(msg.contains(&p.display().to_string()), "got: {msg}");
    assert!(msg.contains("name"), "got: {msg}");
}

#[test]
fn parses_minimal() {
    let p = PackageManifest::parse("[package]\nname = \"foo\"\nversion = \"1.2.3\"\n").unwrap();
    assert_eq!(*p.name(), pn("foo"));
    assert_eq!(*p.version(), ver("1.2.3"));
    assert_eq!(p.build_number(), 0);
}

#[test]
fn parses_build_number() {
    let p = PackageManifest::parse(
        "[package]\nname = \"foo\"\nversion = \"1.2.3\"\n\
         [package.build.config]\nbuild-number = 5\n",
    )
    .unwrap();
    assert_eq!(p.build_number(), 5);
}

#[test]
fn supports_platform_when_workspace_lists_none() {
    let p = PackageManifest::parse("[package]\nname = \"foo\"\nversion = \"1.0\"\n").unwrap();
    assert!(p.supports_platform(Arch::Linux64));
}

#[test]
fn respects_workspace_platforms_list() {
    let p = PackageManifest::parse(
        "[package]\nname = \"foo\"\nversion = \"1.0\"\n\
         [workspace]\nplatforms = [\"linux-64\"]\n",
    )
    .unwrap();
    assert!(p.supports_platform(Arch::Linux64));
    assert!(!p.supports_platform(Arch::LinuxAarch64));
}

#[test]
fn ignores_unknown_keys() {
    PackageManifest::parse(
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n\
         [tasks]\nci = \"test\"\n[dependencies]\nsomething = \"1\"\n",
    )
    .unwrap();
}

fn is_noarch(toml: &str) -> bool {
    PackageManifest::parse(toml).unwrap().is_noarch()
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
fn deps_are_collected_from_every_dep_table() {
    let m = PackageManifest::parse(
        "[dependencies]\nnode = { path = \".\" }\n\
         [package]\nname=\"node\"\nversion=\"1\"\n\
         [package.run-dependencies]\nlib = { path = \"../lib\" }\n\
         [package.host-dependencies]\nmsgs = { path = \"../msgs\" }\n\
         [package.build-dependencies]\ngen = \"==1.2.3\"\n",
    )
    .unwrap();
    let names: Vec<&str> = m.deps().iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["node", "lib", "msgs", "gen"]);
}

// conda-forge really ships these, and conda virtual packages surface with a
// double underscore. A manifest that copies a solved environment's deps in
// must parse, not vanish from discovery behind a warning.
#[test]
fn dep_keys_may_be_underscore_prefixed_conda_packages() {
    let m = PackageManifest::parse(
        "[package]\nname=\"node\"\nversion=\"1\"\n\
         [package.run-dependencies]\n\
         _libgcc_mutex = \"*\"\n_openmp_mutex = \"*\"\n\
         _sysroot_linux-64_curr_repodata_hack = \"*\"\n__cuda = \">=12\"\n",
    )
    .unwrap();
    let names: Vec<&str> = m.deps().iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "__cuda",
            "_libgcc_mutex",
            "_openmp_mutex",
            "_sysroot_linux-64_curr_repodata_hack",
        ]
    );
}

#[test]
fn path_dep_rel_paths_excludes_self_idiom() {
    let t = PackageManifest::parse(
        "[dependencies]\nnode = { path = \".\" }\n\
         [package]\nname=\"node\"\nversion=\"1\"\n\
         [package.run-dependencies]\nlib = { path = \"../lib\" }\nros-kilted-rclpy = \"*\"\n\
         [package.host-dependencies]\nmsgs = { path = \"../msgs\" }\n",
    )
    .unwrap();
    let mut got = t.path_dep_rel_paths();
    got.sort();
    assert_eq!(got, vec!["../lib".to_string(), "../msgs".to_string()]);
}

#[test]
fn exact_pin_version_parses_only_concrete_triples() {
    assert_eq!(exact_pin_version("==2.5.0"), Some(ver("2.5.0")));
    assert_eq!(
        exact_pin_version("==2.5.0-alpha.1"),
        Some(ver("2.5.0-alpha.1"))
    );
    assert_eq!(exact_pin_version(">=2.5.0"), None);
    assert_eq!(exact_pin_version("==2.5.*"), None);
    assert_eq!(exact_pin_version("*"), None);
    assert_eq!(exact_pin_version("==2.5"), None); // not three components
}

#[test]
fn exact_pins_lists_pin_keys_across_tables() {
    let t = PackageManifest::parse(
        "[package]\nname=\"node\"\nversion=\"1\"\n\
         [package.run-dependencies]\nlib = \"==2.5.0\"\nros-kilted-rclpy = \"*\"\n\
         [package.host-dependencies]\nmsgs = \"==1.2.3\"\n",
    )
    .unwrap();
    let mut got: Vec<PackageName> = t.exact_pins().into_iter().map(|(k, _)| k).collect();
    got.sort();
    assert_eq!(got, vec![pn("lib"), pn("msgs")]);
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

#[test]
fn discovers_all_packages_when_no_filter() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_pkg(tmp.path(), "beta");
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 2);
    assert!(result[0].ends_with("alpha/pixi.toml"));
    assert!(result[1].ends_with("beta/pixi.toml"));
}

#[test]
fn discovery_parses_each_manifest_once_into_a_package() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkgs = discover(tmp.path(), None).unwrap();
    let alpha = &pkgs[0];
    assert_eq!(alpha.identity().name, pn("alpha"));
    assert_eq!(alpha.identity().version, ver("1.0.0"));
    assert_eq!(alpha.dir, tmp.path().join("alpha"));
    assert_eq!(alpha.manifest_path, tmp.path().join("alpha/pixi.toml"));
}

#[test]
fn discover_with_filter_returns_single_package() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_pkg(tmp.path(), "beta");
    let result = manifest_paths(&discover(tmp.path(), Some(&pn("alpha"))).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_filter_unknown_package_errors() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let err = discover(tmp.path(), Some(&pn("ghost"))).unwrap_err();
    assert!(err.to_string().contains("ghost"));
}

#[test]
fn discover_returns_root_package_when_pixi_declares_package() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("pixi.toml"),
        "[workspace]\nname = \"mise\"\n[package]\nname = \"mise\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].manifest_path.ends_with("pixi.toml"));
    // Filter matching the root package name also returns it.
    let filtered = discover(tmp.path(), Some(&pn("mise"))).unwrap();
    assert_eq!(filtered.len(), 1);
}

#[test]
fn discover_falls_through_when_root_pixi_is_workspace_only() {
    // A workspace-only root pixi.toml must NOT shadow real subdir packages.
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("pixi.toml"), "[workspace]\nname = \"ws\"\n").unwrap();
    make_pkg(tmp.path(), "alpha");
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_skips_workspace_only_manifest() {
    // deepstream_extensions shape: a dev env for a package published from a
    // hand-authored recipe. It must not break the whole repo's release.
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    make_workspace_only(tmp.path(), "deepstream_extensions");
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn discover_filter_on_workspace_only_manifest_errors() {
    let tmp = TempDir::new().unwrap();
    make_workspace_only(tmp.path(), "devenv");
    let err = discover(tmp.path(), Some(&pn("devenv"))).unwrap_err();
    assert!(err.to_string().contains("no [package] section"));
}

#[test]
fn a_syntactically_broken_sibling_does_not_hide_its_neighbours() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkg = tmp.path().join("broken");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("pixi.toml"), "[workspace\nname = ").unwrap();
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn a_sibling_with_a_type_mismatched_field_does_not_hide_its_neighbours() {
    // Valid TOML, wrong types: `build-number` is a u64 and `platforms` a list
    // in the schema. alpha's release never reads either, so a mistake in
    // broken/ must not make alpha undiscoverable.
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkg = tmp.path().join("broken");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        "[workspace]\nplatforms = \"linux-64\"\n\
         [package]\nname = \"broken\"\nversion = \"1.0.0\"\n\
         [package.build.config]\nbuild-number = \"not-a-number\"\n",
    )
    .unwrap();
    // The manifest really is rejected by the schema...
    assert!(Manifest::read(&pkg.join("pixi.toml")).is_err());
    // ...and discovery survives it.
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn a_sibling_whose_package_key_is_not_a_table_does_not_hide_its_neighbours() {
    // A `package` that is a scalar rather than a table is a malformed manifest
    // — a stray line or a `[[package]]` typo — not a package that names itself
    // unreadably. It must stay a tolerated skip, or one broken file aborts
    // discovery for every sibling beside it.
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkg = tmp.path().join("broken");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("pixi.toml"), "package = \"oops\"\n").unwrap();

    assert!(Manifest::read(&pkg.join("pixi.toml")).is_err());
    let result = manifest_paths(&discover(tmp.path(), None).unwrap());
    assert_eq!(result.len(), 1);
    assert!(result[0].ends_with("alpha/pixi.toml"));
}

#[test]
fn a_sibling_with_no_package_name_is_fatal_to_the_sweep() {
    // The other side of the line: a real `[package]` table that fails to name
    // itself IS a package, so warn-and-skip would silently drop it from a
    // release. It has to be loud.
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkg = tmp.path().join("nameless");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("pixi.toml"), "[package]\nversion = \"1.0.0\"\n").unwrap();

    let err = discover(tmp.path(), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("nameless"), "got: {msg}");
}

// A short conda version is legal in pixi.toml. It must not take the package
// out of a release run, and writing the manifest back must not rewrite it.
#[test]
fn a_two_component_version_is_a_package_and_keeps_its_spelling() {
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("shortver");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        "[package]\nname = \"shortver\"\nversion = \"1.0\"\n",
    )
    .unwrap();
    let found = discover(tmp.path(), None).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].identity().version.to_string(), "1.0");
}

// The opposite of the tolerant sweep: a version we genuinely cannot read
// belongs to a package that exists, so skipping it would drop it from the run
// without failing the run.
#[test]
fn a_sibling_whose_version_is_unreadable_fails_the_sweep_loudly() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    let pkg = tmp.path().join("epoch");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("pixi.toml"),
        "[package]\nname = \"epoch\"\nversion = \"1!1.0.0\"\n",
    )
    .unwrap();
    let err = discover(tmp.path(), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("cannot identify"), "got: {msg}");
    assert!(msg.contains("epoch"), "got: {msg}");
}

#[test]
fn discover_with_a_filter_still_reports_a_malformed_manifest() {
    // An explicit request is answered or refused — never silently empty.
    let tmp = TempDir::new().unwrap();
    let pkg = tmp.path().join("broken");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("pixi.toml"), "[workspace\nname = ").unwrap();
    let err = discover(tmp.path(), Some(&pn("broken"))).unwrap_err();
    assert!(format!("{err:#}").contains("parsing"));
}

#[test]
fn discover_skips_directories_without_pixi_toml() {
    let tmp = TempDir::new().unwrap();
    make_pkg(tmp.path(), "alpha");
    fs::create_dir_all(tmp.path().join("not-a-package")).unwrap();
    let result = discover(tmp.path(), None).unwrap();
    assert_eq!(result.len(), 1);
}

// ---------------------------------------------------------------------------
// Edit view
// ---------------------------------------------------------------------------

#[test]
fn set_package_version_bumps_version_in_package_table() {
    let before = r#"[workspace]
name = "foo"
[package]
name = "foo"
version = "1.0.0"
description = "Test"
"#;
    let after = set_package_version(before, &ver("1.2.3")).unwrap();
    assert!(after.contains(r#"version = "1.2.3""#));
    assert!(!after.contains(r#"version = "1.0.0""#));
    assert!(after.contains(r#"description = "Test""#));
    assert!(after.contains(r#"name = "foo""#));
}

#[test]
fn set_package_version_does_not_touch_workspace_version() {
    let before = "[workspace]\nname = \"foo\"\nversion = \"0.0.0\"\n\
                  [package]\nname = \"foo\"\nversion = \"1.0.0\"\n";
    let after = set_package_version(before, &ver("1.2.3")).unwrap();
    let parsed: toml_edit::DocumentMut = after.parse().unwrap();
    assert_eq!(parsed["workspace"]["version"].as_str(), Some("0.0.0"));
    assert_eq!(parsed["package"]["version"].as_str(), Some("1.2.3"));
}

#[test]
fn set_package_version_errors_when_no_package_table() {
    let err = set_package_version("[workspace]\nname = \"foo\"\n", &ver("1.2.3")).unwrap_err();
    assert!(err.to_string().contains("[package]"));
}

#[test]
fn set_package_version_errors_when_no_version_key() {
    let err = set_package_version("[package]\nname = \"foo\"\n", &ver("1.2.3")).unwrap_err();
    assert!(err.to_string().contains("version"));
}

#[test]
fn set_package_version_preserves_comments_within_package_table() {
    let before = r#"[package]
name = "foo"
# Bump deliberately — this comment must survive
version = "1.0.0"
description = "Test"
"#;
    let after = set_package_version(before, &ver("1.2.3")).unwrap();
    assert!(after.contains("# Bump deliberately"));
    assert!(after.contains(r#"version = "1.2.3""#));
}

/// `set_package_version` is shared with `ci sync-cargo`: Cargo.toml has the
/// same `[package] version` shape, and the lookalike dependency version lines
/// must not move.
#[test]
fn set_package_version_bumps_only_the_package_version_in_cargo_toml() {
    let before = r#"[package]
name = "mise"
version = "1.0.0"
edition = "2021"

[dependencies]
anyhow = "1.0.0"
"#;
    let after = set_package_version(before, &ver("2.3.4")).unwrap();
    let doc: toml_edit::DocumentMut = after.parse().unwrap();
    assert_eq!(doc["package"]["version"].as_str(), Some("2.3.4"));
    assert_eq!(doc["dependencies"]["anyhow"].as_str(), Some("1.0.0"));
}

fn write_tmp_pixi_toml(text: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("pixi.toml");
    fs::write(&path, text).unwrap();
    (tmp, path)
}

#[test]
fn set_build_number_updates_existing_field() {
    let (_tmp, path) = write_tmp_pixi_toml(
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n\
         \n[package.build.config]\nbuild-number = 0\n",
    );
    set_build_number(&path, 5).unwrap();
    let updated = fs::read_to_string(&path).unwrap();
    assert_eq!(PackageManifest::parse(&updated).unwrap().build_number(), 5);
}

#[test]
fn set_build_number_inserts_when_absent() {
    let (_tmp, path) = write_tmp_pixi_toml("[package]\nname = \"foo\"\nversion = \"1.0.0\"\n");
    set_build_number(&path, 2).unwrap();
    let updated = fs::read_to_string(&path).unwrap();
    assert_eq!(PackageManifest::parse(&updated).unwrap().build_number(), 2);
}

#[test]
fn set_build_number_is_idempotent() {
    let (_tmp, path) = write_tmp_pixi_toml(
        "[package]\nname = \"foo\"\nversion = \"1.0.0\"\n\
         \n[package.build.config]\nbuild-number = 0\n",
    );
    set_build_number(&path, 4).unwrap();
    let first = fs::read_to_string(&path).unwrap();
    set_build_number(&path, 4).unwrap();
    let second = fs::read_to_string(&path).unwrap();
    assert_eq!(first, second);
}

#[test]
fn set_build_number_preserves_unrelated_keys_and_comments() {
    let original = r#"# top-of-file comment
[package]
name = "foo"  # inline comment
version = "1.0"

[tasks]
ci = "test"
"#;
    let (_tmp, path) = write_tmp_pixi_toml(original);
    set_build_number(&path, 1).unwrap();
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("# top-of-file comment"), "got: {updated}");
    assert!(updated.contains("# inline comment"), "got: {updated}");
    assert!(updated.contains("ci = \"test\""), "got: {updated}");
    assert!(updated.contains("build-number = 1"), "got: {updated}");
}

#[test]
fn set_build_number_errors_when_package_missing() {
    let (_tmp, path) = write_tmp_pixi_toml("[tasks]\nci = \"test\"\n");
    let err = set_build_number(&path, 1).unwrap_err();
    assert!(format!("{err:#}").contains("missing [package]"));
}

#[test]
fn prepend_channels_front_inserts() {
    let (_tmp, path) = write_tmp_pixi_toml(
        "[workspace]\nname = \"x\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
         [package]\nname = \"x\"\nversion = \"1.0.0\"\n",
    );
    prepend_channels(
        &path,
        &[
            ChannelUrl::parse("file:///out").unwrap(),
            ChannelUrl::parse("file:///local-deps").unwrap(),
        ],
    )
    .unwrap();
    let doc: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
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

fn write_checkout_pkg(root: &Path, name: &str, extra: &str) -> PathBuf {
    let dir = root.join("packages").join(name);
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("pixi.toml");
    fs::write(
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
    let resolved = resolve_path_deps(&consumer, SiblingPinStyle::Range).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, pn("lib"));
    assert_eq!(resolved[0].version, ver("2.5.0"));

    let text = fs::read_to_string(&consumer).unwrap();
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
    // The sibling's own package.name is "lib", but the channel artifact it
    // publishes is "ros-kilted-lib", which is the key the consumer depends on
    // it under. The rewrite must key off the dep key, not the sibling's name.
    write_checkout_pkg(root, "lib", "");
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nros-kilted-lib = { path = \"../lib\" }\n",
    );
    let resolved = resolve_path_deps(&consumer, SiblingPinStyle::Range).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, pn("ros-kilted-lib"));
    assert_eq!(resolved[0].version, ver("2.5.0"));

    let text = fs::read_to_string(&consumer).unwrap();
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
    fs::create_dir_all(&dir).unwrap();
    fs::write(
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
    let err = resolve_path_deps(&consumer, SiblingPinStyle::Range).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("path dep lib"), "got: {msg}");
    assert!(msg.contains("missing field `version`"), "got: {msg}");
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
    let resolved = resolve_path_deps(&consumer, SiblingPinStyle::Range).unwrap();
    assert_eq!(resolved[0].version, ver("2.5.0"));
}

#[test]
fn range_pin_derives_major_cap() {
    assert_eq!(ver("2.5.0").range_pin(), ">=2.5.0,<3");
    assert_eq!(ver("1.24.0-alpha.2").range_pin(), ">=1.24.0-alpha.2,<2");
    assert_eq!(ver("0.3.1").range_pin(), ">=0.3.1,<1");
}

#[test]
fn resolve_path_deps_exact_style_writes_lockstep_pins() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_checkout_pkg(root, "lib", "");
    let consumer = write_checkout_pkg(
        root,
        "node",
        "[package.run-dependencies]\nlib = { path = \"../lib\" }\n",
    );
    let resolved = resolve_path_deps(&consumer, SiblingPinStyle::Exact).unwrap();
    assert_eq!(resolved[0].version, ver("2.5.0"));

    let text = fs::read_to_string(&consumer).unwrap();
    assert!(text.contains("lib = \"==2.5.0\""), "rewritten: {text}");
    assert!(
        text.contains("node = { path = \".\" }"),
        "self idiom untouched: {text}"
    );
}
