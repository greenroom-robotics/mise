use super::*;

fn entry<'a>() -> Entry<'a> {
    Entry {
        package: "foo",
        url: "https://example.invalid/foo.git",
        tag: "1.2.3",
        version: "1.2.3",
    }
}

#[test]
fn upsert_into_empty_yields_fresh_block() {
    let out = upsert_text("", &entry()).unwrap();
    assert_eq!(
        out,
        "foo:\n  url: https://example.invalid/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    );
}

#[test]
fn upsert_appends_new_entry_with_blank_line_separator() {
    let existing = "bar:\n  url: https://example.invalid/bar.git\n  tag: 0.1.0\n  version: 0.1.0\n";
    let out = upsert_text(existing, &entry()).unwrap();
    assert!(out.starts_with(existing));
    assert!(out.contains("\n\nfoo:\n"));
    assert!(out.ends_with(
        "foo:\n  url: https://example.invalid/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
}

#[test]
fn upsert_replaces_existing_block_in_place() {
    let existing = "\
foo:
  url: https://example.invalid/foo.git
  tag: 1.0.0
  version: 1.0.0
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry()).unwrap();
    // foo's block is replaced with the new tag/version
    assert!(out.contains(
        "foo:\n  url: https://example.invalid/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
    // bar's block is untouched
    assert!(out.contains(
        "bar:\n  url: https://example.invalid/bar.git\n  tag: 0.1.0\n  version: 0.1.0\n"
    ));
    // No duplicate foo: line
    assert_eq!(
        out.matches("\nfoo:").count() + out.starts_with("foo:") as usize,
        1
    );
}

#[test]
fn upsert_preserves_comments_outside_the_block() {
    let existing = "\
# Top of file comment
foo:
  url: https://example.invalid/foo.git
  tag: 1.0.0
  version: 1.0.0

# Notes about bar — important context
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry()).unwrap();
    assert!(out.contains("# Top of file comment"));
    assert!(out.contains("# Notes about bar — important context"));
}

#[test]
fn upsert_replaces_block_with_extra_optional_fields() {
    // Real-world entries sometimes carry additional_folder / branch /
    // manifest_file. On upsert we replace with the canonical four-field
    // shape — this is acceptable because callers know the canonical shape.
    let existing = "\
foo:
  url: https://example.invalid/foo.git
  tag: 1.0.0
  version: 1.0.0
  additional_folder: packages/foo
  manifest_file: package.xml
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry()).unwrap();
    // foo replaced, with the optional fields gone:
    assert!(!out.contains("additional_folder"));
    assert!(out.contains(
        "foo:\n  url: https://example.invalid/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
    // bar still intact:
    assert!(out.contains("bar:\n  url: https://example.invalid/bar.git"));
}

#[test]
fn upsert_replaces_a_block_whose_body_continues_past_a_column_zero_comment() {
    // The reader (`field_of`) and the writer (`section_bounds`) must agree
    // that `# a note` is interior here: if the writer stopped at the comment
    // it would splice in a fresh block and strand the old `version:` beside
    // it, and the reader would keep reporting the stale value.
    let existing = "\
foo:
  url: https://example.invalid/foo.git
  tag: 1.0.0
# a note
  version: 1.0.0
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
";
    assert_eq!(
        yaml_block::field_of(existing, "foo", "version"),
        Some("1.0.0")
    );

    let out = upsert_text(existing, &entry()).unwrap();
    assert_eq!(
        out,
        "\
foo:
  url: https://example.invalid/foo.git
  tag: 1.2.3
  version: 1.2.3
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
"
    );
    assert_eq!(yaml_block::field_of(&out, "foo", "version"), Some("1.2.3"));
    assert_eq!(out.matches("version:").count(), 1);
}

#[test]
fn upsert_keeps_a_comment_that_captions_the_next_block() {
    let existing = "\
foo:
  url: https://example.invalid/foo.git
  tag: 1.0.0
# vendored fork — do not bump
bar:
  url: https://example.invalid/bar.git
";
    let out = upsert_text(existing, &entry()).unwrap();
    assert!(out.contains("# vendored fork — do not bump\nbar:\n"));
    assert!(out.contains("  tag: 1.2.3\n"));
}

#[test]
fn upsert_reemits_crlf_line_endings() {
    let existing = "foo:\r\n  url: old\r\n  tag: 0.1.0\r\nbar:\r\n  url: keep\r\n";
    let out = upsert_text(existing, &entry()).unwrap();
    assert_eq!(
        out,
        "foo:\r\n  url: https://example.invalid/foo.git\r\n  tag: 1.2.3\r\n  \
         version: 1.2.3\r\nbar:\r\n  url: keep\r\n"
    );
}

const VENDORED_FIXTURE: &str = r#"# yaml-language-server: $schema=https://example.com/schema.json
#
# Vendor recipe for foo — header comment must survive.

package:
  name: foo
  version: 1.2.3

source:
  git: https://github.com/example/foo.git
  rev: 4bcfd421c52387b3f7872b23e60059e521176f35

build:
  number: 2
  script: ${{ '$RECIPE_DIR/build.sh' }}

requirements:
  host:
    - bar ==1.1.3
"#;

#[test]
fn vendored_updates_version_rev_and_resets_build_number() {
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        "1.3.0",
        "1111111111111111111111111111111111111111",
    )
    .unwrap();
    assert!(out.contains("  version: 1.3.0"));
    assert!(out.contains("  rev: 1111111111111111111111111111111111111111"));
    assert!(out.contains("  number: 0"));
    // Everything else untouched.
    assert!(out.contains("# Vendor recipe for foo — header comment must survive."));
    assert!(out.contains("  script: ${{ '$RECIPE_DIR/build.sh' }}"));
    assert!(out.contains("    - bar ==1.1.3"));
    assert!(out.contains("  name: foo"));
}

#[test]
fn vendored_noop_when_version_and_rev_match() {
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        "1.2.3",
        "4bcfd421c52387b3f7872b23e60059e521176f35",
    )
    .unwrap();
    assert_eq!(out, VENDORED_FIXTURE);
}

#[test]
fn vendored_keeps_build_number_when_version_unchanged() {
    // Same version, new rev (re-tag): rev updates, number stays manual.
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        "1.2.3",
        "2222222222222222222222222222222222222222",
    )
    .unwrap();
    assert!(out.contains("  rev: 2222222222222222222222222222222222222222"));
    assert!(out.contains("  number: 2"));
}

#[test]
fn vendored_errors_when_rev_missing() {
    let no_rev = VENDORED_FIXTURE.replace("  rev: 4bcfd421c52387b3f7872b23e60059e521176f35\n", "");
    let err = mutate_vendored_recipe(&no_rev, "1.3.0", "1111111111111111111111111111111111111111")
        .unwrap_err();
    assert!(err.to_string().contains("source.rev"));
}

#[test]
fn vendored_errors_when_version_missing() {
    let no_version = VENDORED_FIXTURE.replace("  version: 1.2.3\n", "");
    let err = mutate_vendored_recipe(
        &no_version,
        "1.3.0",
        "1111111111111111111111111111111111111111",
    )
    .unwrap_err();
    assert!(err.to_string().contains("package.version"));
}

#[test]
fn vendored_errors_when_build_number_missing() {
    let no_number = VENDORED_FIXTURE.replace("  number: 2\n", "");
    let err = mutate_vendored_recipe(
        &no_number,
        "1.3.0",
        "1111111111111111111111111111111111111111",
    )
    .unwrap_err();
    assert!(err.to_string().contains("build.number"));
}

#[test]
fn vendored_errors_on_templated_version() {
    let templated = VENDORED_FIXTURE.replace("  version: 1.2.3\n", "  version: ${{ some_var }}\n");
    let err = mutate_vendored_recipe(
        &templated,
        "1.3.0",
        "1111111111111111111111111111111111111111",
    )
    .unwrap_err();
    assert!(err.to_string().contains("templated"));
}

#[test]
fn vendored_preserves_trailing_newline() {
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        "1.3.0",
        "1111111111111111111111111111111111111111",
    )
    .unwrap();
    assert!(out.ends_with('\n'));
}

#[test]
fn vendored_reemits_crlf_line_endings() {
    let crlf = VENDORED_FIXTURE.replace('\n', "\r\n");
    let out =
        mutate_vendored_recipe(&crlf, "1.3.0", "1111111111111111111111111111111111111111").unwrap();
    // Every LF is still part of a CRLF pair — no line was silently converted.
    assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
    assert!(out.contains("  version: 1.3.0\r\n"));
    // A no-op edit is byte-exact.
    let noop =
        mutate_vendored_recipe(&out, "1.3.0", "1111111111111111111111111111111111111111").unwrap();
    assert_eq!(noop, out);
}

#[test]
fn vendored_preserves_a_missing_trailing_newline() {
    let no_nl = VENDORED_FIXTURE.trim_end_matches('\n');
    let out =
        mutate_vendored_recipe(no_nl, "1.3.0", "1111111111111111111111111111111111111111").unwrap();
    assert!(!out.ends_with('\n'));
}

const PIXI_FIXTURE: &str = "\
# Header comment must survive.
packages:
  - name: alpha
    url: https://github.com/example/alpha
    ref: main

  - name: beta
    url: https://github.com/example/beta.git
    rev: 1111111111111111111111111111111111111111
    subdir: packages/beta

  - name: gamma
    url: https://github.com/example/gamma
    rev: 2222222222222222222222222222222222222222
";

#[test]
fn pixi_replaces_ref_with_rev_on_existing_entry() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        "alpha",
        "https://github.com/example/alpha.git",
        "3333333333333333333333333333333333333333",
        None,
    )
    .unwrap();
    assert!(!out.contains("ref: main"));
    assert!(out.contains("rev: 3333333333333333333333333333333333333333"));
    assert!(out.contains("url: https://github.com/example/alpha.git"));
    // Other entries untouched.
    assert!(out.contains("subdir: packages/beta"));
    assert!(out.contains("# Header comment"));
}

#[test]
fn pixi_updates_subdir_when_passed() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        "beta",
        "https://github.com/example/beta.git",
        "4444444444444444444444444444444444444444",
        Some("packages/beta-new"),
    )
    .unwrap();
    assert!(out.contains("subdir: packages/beta-new"));
    assert!(!out.contains("subdir: packages/beta\n"));
    assert!(out.contains("rev: 4444444444444444444444444444444444444444"));
}

#[test]
fn pixi_appends_new_entry_when_absent() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        "delta",
        "https://github.com/example/delta.git",
        "5555555555555555555555555555555555555555",
        Some("packages/delta"),
    )
    .unwrap();
    assert!(out.contains("- name: alpha"));
    assert!(out.contains("- name: delta"));
    assert!(out.contains("url: https://github.com/example/delta.git"));
    assert!(out.contains("rev: 5555555555555555555555555555555555555555"));
    assert!(out.contains("subdir: packages/delta"));
}

#[test]
fn pixi_appends_without_subdir() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        "epsilon",
        "https://github.com/example/epsilon",
        "6666666666666666666666666666666666666666",
        None,
    )
    .unwrap();
    assert!(out.contains("- name: epsilon"));
    assert!(!out.lines().any(|l| l.trim() == "subdir:"));
}

#[test]
fn pixi_reemits_crlf_line_endings() {
    let crlf = PIXI_FIXTURE.replace('\n', "\r\n");
    let out = mutate_pixi_entry(
        &crlf,
        "alpha",
        "https://github.com/example/alpha.git",
        "3333333333333333333333333333333333333333",
        None,
    )
    .unwrap();
    assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
    assert!(out.contains("    rev: 3333333333333333333333333333333333333333\r\n"));
    // Only alpha's lines differ; the rest of the file is byte-identical.
    assert!(out.ends_with("  - name: gamma\r\n    url: https://github.com/example/gamma\r\n    rev: 2222222222222222222222222222222222222222\r\n"));
}

#[test]
fn pixi_preserves_a_missing_trailing_newline() {
    let no_nl = PIXI_FIXTURE.trim_end_matches('\n');
    let out = mutate_pixi_entry(
        no_nl,
        "alpha",
        "https://github.com/example/alpha.git",
        "3333333333333333333333333333333333333333",
        None,
    )
    .unwrap();
    assert!(!out.ends_with('\n'));
}

#[test]
fn pixi_preserves_tab_indented_sub_keys_it_does_not_own() {
    let text = "packages:\n  - name: alpha\n    url: old\n\tcomment_key: kept\n    rev: aaaa\n";
    let out = mutate_pixi_entry(text, "alpha", "new-url", "bbbb", None).unwrap();
    assert!(out.contains("\tcomment_key: kept\n"));
    assert!(out.contains("    url: new-url\n"));
    assert!(out.contains("    rev: bbbb\n"));
}

#[test]
fn pixi_preserves_blank_line_between_items() {
    // After mutating `alpha`, the blank line that originally separated it
    // from the next item must remain between items, NOT migrate inside
    // alpha's block.
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        "alpha",
        "https://github.com/example/alpha.git",
        "7777777777777777777777777777777777777777",
        None,
    )
    .unwrap();
    let lines: Vec<&str> = out.lines().collect();
    let alpha_idx = lines
        .iter()
        .position(|l| l.trim() == "- name: alpha")
        .unwrap();
    let beta_idx = lines
        .iter()
        .position(|l| l.trim() == "- name: beta")
        .unwrap();
    // Inside alpha's block (from header to next blank/item) there must be NO blank line.
    let alpha_block_end = lines[alpha_idx + 1..beta_idx]
        .iter()
        .position(|l| l.trim().is_empty())
        .map(|p| alpha_idx + 1 + p)
        .unwrap_or(beta_idx);
    for line in &lines[alpha_idx + 1..alpha_block_end] {
        assert!(
            !line.trim().is_empty(),
            "alpha block should be contiguous, got blank inside: {out}"
        );
    }
    // And the blank between alpha and beta must still exist.
    assert!(
        lines[alpha_block_end..beta_idx]
            .iter()
            .any(|l| l.trim().is_empty()),
        "blank line between items lost: {out}"
    );
}

fn write(root: &std::path::Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn apply_release_patches_vendored_recipe_when_present() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "vendor_recipes/is-core/recipe.yaml",
        "package:\n  name: is-core\n  version: 1.0.0\n\nsource:\n  git: https://github.com/example/is-core.git\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 2\n",
    );
    let applied = apply_release(
        root,
        "is-core",
        "https://github.com/example/is-core.git",
        "v1.1.0",
        "1.1.0",
        "1111111111111111111111111111111111111111",
        None,
    )
    .unwrap();
    assert_eq!(
        applied.path,
        std::path::Path::new("vendor_recipes/is-core/recipe.yaml")
    );
    // Old ref is the source.rev the recipe pinned before this release.
    assert_eq!(
        applied.old_ref,
        Some(OldRef::Rev(
            "0000000000000000000000000000000000000000".into()
        ))
    );
    let out = std::fs::read_to_string(root.join("vendor_recipes/is-core/recipe.yaml")).unwrap();
    assert!(
        out.contains("version: 1.1.0")
            && out.contains("rev: 1111111111111111111111111111111111111111")
            && out.contains("number: 0")
    );
}

#[test]
fn apply_release_updates_existing_pixi_native_entry() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "rebuild_epoch: 0\n\npackages:\n  - name: mise\n    url: https://github.com/greenroom-robotics/mise\n    rev: 0000000000000000000000000000000000000000\n",
    );
    let applied = apply_release(
        root,
        "mise",
        "https://github.com/greenroom-robotics/mise",
        "v4.4.0",
        "4.4.0",
        "2222222222222222222222222222222222222222",
        None,
    )
    .unwrap();
    assert_eq!(
        applied.path,
        std::path::Path::new("pixi_native_packages.yaml")
    );
    assert_eq!(
        applied.old_ref,
        Some(OldRef::Rev(
            "0000000000000000000000000000000000000000".into()
        ))
    );
    let out = std::fs::read_to_string(root.join("pixi_native_packages.yaml")).unwrap();
    assert!(out.contains("rev: 2222222222222222222222222222222222222222"));
    assert!(!root.join("rosdistro_additional_recipes.yaml").exists());
}

#[test]
fn apply_release_updates_existing_rosdistro_entry() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "rebuild_epoch: 0\n\npackages:\n",
    );
    write(
        root,
        "rosdistro_additional_recipes.yaml",
        "foo_pkg:\n  url: https://github.com/example/foo_pkg.git\n  tag: 0.1.0\n  version: 0.1.0\n",
    );
    let applied = apply_release(
        root,
        "foo_pkg",
        "https://github.com/example/foo_pkg.git",
        "v0.2.0",
        "0.2.0",
        "3333333333333333333333333333333333333333",
        Some("packages/foo_pkg"),
    )
    .unwrap();
    assert_eq!(
        applied.path,
        std::path::Path::new("rosdistro_additional_recipes.yaml")
    );
    // rosdistro pins a tag; old ref is the previous tag.
    assert_eq!(applied.old_ref, Some(OldRef::Tag("0.1.0".into())));
    let out = std::fs::read_to_string(root.join("rosdistro_additional_recipes.yaml")).unwrap();
    assert!(out.contains("tag: v0.2.0") && out.contains("version: 0.2.0"));
}

#[test]
fn apply_release_defaults_brand_new_package_to_pixi_native() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "rebuild_epoch: 0\n\npackages:\n  - name: existing\n    url: https://example.invalid/existing\n    rev: 0000000000000000000000000000000000000000\n",
    );
    let applied = apply_release(
        root,
        "newpkg",
        "https://github.com/example/newpkg.git",
        "v1.0.0",
        "1.0.0",
        "4444444444444444444444444444444444444444",
        Some("packages/newpkg"),
    )
    .unwrap();
    assert_eq!(
        applied.path,
        std::path::Path::new("pixi_native_packages.yaml")
    );
    // Brand-new package had no prior pin.
    assert_eq!(applied.old_ref, None);
    let out = std::fs::read_to_string(root.join("pixi_native_packages.yaml")).unwrap();
    assert!(out.contains("- name: newpkg"));
    assert!(out.contains("rev: 4444444444444444444444444444444444444444"));
    assert!(out.contains("subdir: packages/newpkg"));
}

#[test]
fn apply_release_errors_when_pixi_native_absent() {
    // Brand-new package, no vendored recipe, and no pixi_native_packages.yaml
    // to append to -> loud error rather than silently writing nothing.
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    let err = apply_release(
        root,
        "newpkg",
        "https://github.com/example/newpkg.git",
        "v1.0.0",
        "1.0.0",
        "5555555555555555555555555555555555555555",
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("pixi_native_packages.yaml"));
}

#[test]
fn apply_release_resolves_hyphenated_vendored_dir() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "vendor_recipes/deepstream-extensions/recipe.yaml",
        "package:\n  name: deepstream-extensions\n  version: 1.0.1\n\nsource:\n  git: https://github.com/example/pp.git\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 0\n",
    );
    // Called with the underscore ROS/tag name; must resolve the hyphen dir.
    let applied = apply_release(
        root,
        "deepstream_extensions",
        "https://github.com/example/pp.git",
        "v1.1.0",
        "1.1.0",
        "1111111111111111111111111111111111111111",
        None,
    )
    .unwrap();
    assert_eq!(
        applied.path,
        std::path::Path::new("vendor_recipes/deepstream-extensions/recipe.yaml")
    );
    let out =
        std::fs::read_to_string(root.join("vendor_recipes/deepstream-extensions/recipe.yaml"))
            .unwrap();
    assert!(out.contains("version: 1.1.0"));
    assert!(out.contains("rev: 1111111111111111111111111111111111111111"));
    assert!(out.contains("number: 0"));
}
