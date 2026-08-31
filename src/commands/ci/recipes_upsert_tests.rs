use super::*;
use crate::types::{GithubRepoUrl, PackageName, Sha40, Version};

fn pkg(s: &str) -> PackageName {
    PackageName::new(s).unwrap()
}

fn ver(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn sha(s: &str) -> Sha40 {
    Sha40::new(s).unwrap()
}

fn url(s: &str) -> GithubRepoUrl {
    GithubRepoUrl::parse_remote(s).unwrap()
}

fn entry<'a>(package: &'a PackageName, repo: &'a GithubRepoUrl, version: &'a Version) -> Entry<'a> {
    Entry {
        package,
        url: repo,
        tag: "1.2.3",
        version,
    }
}

fn foo_entry() -> (PackageName, GithubRepoUrl, Version) {
    (
        pkg("foo"),
        url("https://github.com/example/foo.git"),
        ver("1.2.3"),
    )
}

#[test]
fn upsert_into_empty_yields_fresh_block() {
    let (p, u, v) = foo_entry();
    let out = upsert_text("", &entry(&p, &u, &v)).unwrap();
    assert_eq!(
        out,
        "foo:\n  url: https://github.com/example/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    );
}

#[test]
fn upsert_appends_new_entry_with_blank_line_separator() {
    let (p, u, v) = foo_entry();
    let existing = "bar:\n  url: https://example.invalid/bar.git\n  tag: 0.1.0\n  version: 0.1.0\n";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert!(out.starts_with(existing));
    assert!(out.contains("\n\nfoo:\n"));
    assert!(out.ends_with(
        "foo:\n  url: https://github.com/example/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
}

#[test]
fn upsert_replaces_existing_block_in_place() {
    let (p, u, v) = foo_entry();
    let existing = "\
foo:
  url: https://github.com/example/foo.git
  tag: 1.0.0
  version: 1.0.0
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert!(out.contains(
        "foo:\n  url: https://github.com/example/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
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
    let (p, u, v) = foo_entry();
    let existing = "\
# Top of file comment
foo:
  url: https://github.com/example/foo.git
  tag: 1.0.0
  version: 1.0.0

# Notes about bar — important context
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert!(out.contains("# Top of file comment"));
    assert!(out.contains("# Notes about bar — important context"));
}

#[test]
fn upsert_replaces_block_with_extra_optional_fields() {
    let (p, u, v) = foo_entry();
    // Entries sometimes carry extra optional fields; upsert deliberately
    // replaces the block with the canonical four-field shape.
    let existing = "\
foo:
  url: https://github.com/example/foo.git
  tag: 1.0.0
  version: 1.0.0
  additional_folder: packages/foo
  manifest_file: package.xml
bar:
  url: https://example.invalid/bar.git
  tag: 0.1.0
  version: 0.1.0
";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    // foo replaced, with the optional fields gone:
    assert!(!out.contains("additional_folder"));
    assert!(out.contains(
        "foo:\n  url: https://github.com/example/foo.git\n  tag: 1.2.3\n  version: 1.2.3\n"
    ));
    // bar still intact:
    assert!(out.contains("bar:\n  url: https://example.invalid/bar.git"));
}

#[test]
fn upsert_replaces_a_block_whose_body_continues_past_a_column_zero_comment() {
    let (p, u, v) = foo_entry();
    // The reader (`field_of`) and the writer (`section_bounds`) must agree
    // that `# a note` is interior here: if the writer stopped at the comment
    // it would splice in a fresh block and strand the old `version:` beside
    // it, and the reader would keep reporting the stale value.
    let existing = "\
foo:
  url: https://github.com/example/foo.git
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

    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert_eq!(
        out,
        "\
foo:
  url: https://github.com/example/foo.git
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
    let (p, u, v) = foo_entry();
    let existing = "\
foo:
  url: https://github.com/example/foo.git
  tag: 1.0.0
# vendored fork — do not bump
bar:
  url: https://example.invalid/bar.git
";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert!(out.contains("# vendored fork — do not bump\nbar:\n"));
    assert!(out.contains("  tag: 1.2.3\n"));
}

#[test]
fn upsert_reemits_crlf_line_endings() {
    let (p, u, v) = foo_entry();
    let existing = "foo:\r\n  url: old\r\n  tag: 0.1.0\r\nbar:\r\n  url: keep\r\n";
    let out = upsert_text(existing, &entry(&p, &u, &v)).unwrap();
    assert_eq!(
        out,
        "foo:\r\n  url: https://github.com/example/foo.git\r\n  tag: 1.2.3\r\n  \
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
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap();
    assert!(out.contains("  version: 1.3.0"));
    assert!(out.contains("  rev: 1111111111111111111111111111111111111111"));
    assert!(out.contains("  number: 0"));
    assert!(out.contains("# Vendor recipe for foo — header comment must survive."));
    assert!(out.contains("  script: ${{ '$RECIPE_DIR/build.sh' }}"));
    assert!(out.contains("    - bar ==1.1.3"));
    assert!(out.contains("  name: foo"));
}

#[test]
fn vendored_noop_when_version_and_rev_match() {
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        &ver("1.2.3"),
        &sha("4bcfd421c52387b3f7872b23e60059e521176f35"),
    )
    .unwrap();
    assert_eq!(out, VENDORED_FIXTURE);
}

#[test]
fn vendored_keeps_build_number_when_version_unchanged() {
    // Same version, new rev (re-tag): rev updates, number stays manual.
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        &ver("1.2.3"),
        &sha("2222222222222222222222222222222222222222"),
    )
    .unwrap();
    assert!(out.contains("  rev: 2222222222222222222222222222222222222222"));
    assert!(out.contains("  number: 2"));
}

#[test]
fn vendored_errors_when_rev_missing() {
    let no_rev = VENDORED_FIXTURE.replace("  rev: 4bcfd421c52387b3f7872b23e60059e521176f35\n", "");
    let err = mutate_vendored_recipe(
        &no_rev,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("source.rev"));
}

#[test]
fn vendored_errors_when_version_missing() {
    let no_version = VENDORED_FIXTURE.replace("  version: 1.2.3\n", "");
    let err = mutate_vendored_recipe(
        &no_version,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("package.version"));
}

#[test]
fn vendored_errors_when_build_number_missing() {
    let no_number = VENDORED_FIXTURE.replace("  number: 2\n", "");
    let err = mutate_vendored_recipe(
        &no_number,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("build.number"));
}

#[test]
fn vendored_errors_on_templated_version() {
    let templated = VENDORED_FIXTURE.replace("  version: 1.2.3\n", "  version: ${{ some_var }}\n");
    let err = mutate_vendored_recipe(
        &templated,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("templated"));
}

#[test]
fn vendored_preserves_trailing_newline() {
    let out = mutate_vendored_recipe(
        VENDORED_FIXTURE,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap();
    assert!(out.ends_with('\n'));
}

#[test]
fn vendored_reemits_crlf_line_endings() {
    let crlf = VENDORED_FIXTURE.replace('\n', "\r\n");
    let out = mutate_vendored_recipe(
        &crlf,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap();
    // Every LF is still part of a CRLF pair — no line was silently converted.
    assert_eq!(out.matches('\n').count(), out.matches("\r\n").count());
    assert!(out.contains("  version: 1.3.0\r\n"));
    // A no-op edit is byte-exact.
    let noop = mutate_vendored_recipe(
        &out,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap();
    assert_eq!(noop, out);
}

#[test]
fn vendored_preserves_a_missing_trailing_newline() {
    let no_nl = VENDORED_FIXTURE.trim_end_matches('\n');
    let out = mutate_vendored_recipe(
        no_nl,
        &ver("1.3.0"),
        &sha("1111111111111111111111111111111111111111"),
    )
    .unwrap();
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
        &pkg("alpha"),
        &url("https://github.com/example/alpha.git"),
        &sha("3333333333333333333333333333333333333333"),
        None,
        false,
    )
    .unwrap();
    assert!(!out.contains("ref: main"));
    assert!(out.contains("rev: 3333333333333333333333333333333333333333"));
    assert!(out.contains("url: https://github.com/example/alpha.git"));
    assert!(out.contains("subdir: packages/beta"));
    assert!(out.contains("# Header comment"));
}

#[test]
fn pixi_updates_subdir_when_passed() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        &pkg("beta"),
        &url("https://github.com/example/beta.git"),
        &sha("4444444444444444444444444444444444444444"),
        Some("packages/beta-new"),
        false,
    )
    .unwrap();
    assert!(out.contains("subdir: packages/beta-new"));
    assert!(!out.contains("subdir: packages/beta\n"));
    assert!(out.contains("rev: 4444444444444444444444444444444444444444"));
}

#[test]
fn pixi_lfs_is_authoritative_on_existing_and_new_entries() {
    let on = mutate_pixi_entry(
        PIXI_FIXTURE,
        &pkg("beta"),
        &url("https://github.com/example/beta.git"),
        &sha("4444444444444444444444444444444444444444"),
        Some("packages/beta"),
        true,
    )
    .unwrap();
    assert!(on.contains("    lfs: true"));

    let off = mutate_pixi_entry(
        &on,
        &pkg("beta"),
        &url("https://github.com/example/beta.git"),
        &sha("4444444444444444444444444444444444444444"),
        Some("packages/beta"),
        false,
    )
    .unwrap();
    assert!(!off.contains("lfs:"));

    let appended = mutate_pixi_entry(
        PIXI_FIXTURE,
        &pkg("delta"),
        &url("https://github.com/example/delta.git"),
        &sha("5555555555555555555555555555555555555555"),
        None,
        true,
    )
    .unwrap();
    assert!(appended.contains("- name: delta"));
    assert!(appended.contains("    lfs: true"));
}

#[test]
fn pixi_appends_new_entry_when_absent() {
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        &pkg("delta"),
        &url("https://github.com/example/delta.git"),
        &sha("5555555555555555555555555555555555555555"),
        Some("packages/delta"),
        false,
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
        &pkg("epsilon"),
        &url("https://github.com/example/epsilon"),
        &sha("6666666666666666666666666666666666666666"),
        None,
        false,
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
        &pkg("alpha"),
        &url("https://github.com/example/alpha.git"),
        &sha("3333333333333333333333333333333333333333"),
        None,
        false,
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
        &pkg("alpha"),
        &url("https://github.com/example/alpha.git"),
        &sha("3333333333333333333333333333333333333333"),
        None,
        false,
    )
    .unwrap();
    assert!(!out.ends_with('\n'));
}

#[test]
fn pixi_preserves_tab_indented_sub_keys_it_does_not_own() {
    let text = "packages:\n  - name: alpha\n    url: old\n\tcomment_key: kept\n    rev: aaaa\n";
    let out = mutate_pixi_entry(
        text,
        &pkg("alpha"),
        &url("https://github.com/example/alpha.git"),
        &sha("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        None,
        false,
    )
    .unwrap();
    assert!(out.contains("\tcomment_key: kept\n"));
    assert!(out.contains("    url: https://github.com/example/alpha.git\n"));
    assert!(out.contains("    rev: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n"));
}

#[test]
fn pixi_preserves_blank_line_between_items() {
    // After mutating `alpha`, the blank line that originally separated it
    // from the next item must remain between items, NOT migrate inside
    // alpha's block.
    let out = mutate_pixi_entry(
        PIXI_FIXTURE,
        &pkg("alpha"),
        &url("https://github.com/example/alpha.git"),
        &sha("7777777777777777777777777777777777777777"),
        None,
        false,
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

/// Route then apply, the pair a release actually runs. Returns the file the
/// target writes and the pin it replaced.
fn route_and_apply(
    root: &std::path::Path,
    package: &PackageName,
    url: &GithubRepoUrl,
    tag: &str,
    version: &Version,
    sha: &Sha40,
    subdir: Option<&str>,
) -> anyhow::Result<(std::path::PathBuf, Option<OldRef>)> {
    let target = route(
        root,
        package,
        url,
        tag,
        version,
        sha,
        PixiEntryOpts { subdir, lfs: false },
    )?;
    let path = target.rel_path();
    Ok((path, apply(root, &target)?))
}

#[test]
fn release_patches_vendored_recipe_when_present() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "vendor_recipes/is-core/recipe.yaml",
        "package:\n  name: is-core\n  version: 1.0.0\n\nsource:\n  git: https://github.com/example/is-core.git\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 2\n",
    );
    let (path, old_ref) = route_and_apply(
        root,
        &pkg("is-core"),
        &url("https://github.com/example/is-core.git"),
        "v1.1.0",
        &ver("1.1.0"),
        &sha("1111111111111111111111111111111111111111"),
        None,
    )
    .unwrap();
    assert_eq!(
        path,
        std::path::Path::new("vendor_recipes/is-core/recipe.yaml")
    );
    assert_eq!(
        old_ref,
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
fn release_updates_existing_pixi_native_entry() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "rebuild_epoch: 0\n\npackages:\n  - name: mise\n    url: https://github.com/greenroom-robotics/mise\n    rev: 0000000000000000000000000000000000000000\n",
    );
    let (path, old_ref) = route_and_apply(
        root,
        &pkg("mise"),
        &url("https://github.com/greenroom-robotics/mise"),
        "v4.4.0",
        &ver("4.4.0"),
        &sha("2222222222222222222222222222222222222222"),
        None,
    )
    .unwrap();
    assert_eq!(path, std::path::Path::new("pixi_native_packages.yaml"));
    assert_eq!(
        old_ref,
        Some(OldRef::Rev(
            "0000000000000000000000000000000000000000".into()
        ))
    );
    let out = std::fs::read_to_string(root.join("pixi_native_packages.yaml")).unwrap();
    assert!(out.contains("rev: 2222222222222222222222222222222222222222"));
    assert!(!root.join("rosdistro_additional_recipes.yaml").exists());
}

#[test]
fn release_updates_existing_rosdistro_entry() {
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
    let (path, old_ref) = route_and_apply(
        root,
        &pkg("foo_pkg"),
        &url("https://github.com/example/foo_pkg.git"),
        "v0.2.0",
        &ver("0.2.0"),
        &sha("3333333333333333333333333333333333333333"),
        Some("packages/foo_pkg"),
    )
    .unwrap();
    assert_eq!(
        path,
        std::path::Path::new("rosdistro_additional_recipes.yaml")
    );
    assert_eq!(old_ref, Some(OldRef::Tag("0.1.0".into())));
    let out = std::fs::read_to_string(root.join("rosdistro_additional_recipes.yaml")).unwrap();
    assert!(out.contains("tag: v0.2.0") && out.contains("version: 0.2.0"));
}

#[test]
fn release_defaults_brand_new_package_to_pixi_native() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "rebuild_epoch: 0\n\npackages:\n  - name: existing\n    url: https://example.invalid/existing\n    rev: 0000000000000000000000000000000000000000\n",
    );
    let (path, old_ref) = route_and_apply(
        root,
        &pkg("newpkg"),
        &url("https://github.com/example/newpkg.git"),
        "v1.0.0",
        &ver("1.0.0"),
        &sha("4444444444444444444444444444444444444444"),
        Some("packages/newpkg"),
    )
    .unwrap();
    assert_eq!(path, std::path::Path::new("pixi_native_packages.yaml"));
    assert_eq!(old_ref, None);
    let out = std::fs::read_to_string(root.join("pixi_native_packages.yaml")).unwrap();
    assert!(out.contains("- name: newpkg"));
    assert!(out.contains("rev: 4444444444444444444444444444444444444444"));
    assert!(out.contains("subdir: packages/newpkg"));
}

#[test]
fn release_errors_when_pixi_native_absent() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    let err = route_and_apply(
        root,
        &pkg("newpkg"),
        &url("https://github.com/example/newpkg.git"),
        "v1.0.0",
        &ver("1.0.0"),
        &sha("5555555555555555555555555555555555555555"),
        None,
    )
    .unwrap_err();
    assert!(err.to_string().contains("pixi_native_packages.yaml"));
}

#[test]
fn release_resolves_hyphenated_vendored_dir() {
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "vendor_recipes/deepstream-extensions/recipe.yaml",
        "package:\n  name: deepstream-extensions\n  version: 1.0.1\n\nsource:\n  git: https://github.com/example/pp.git\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 0\n",
    );
    // Called with the underscore ROS/tag name; must resolve the hyphen dir.
    let (path, _old_ref) = route_and_apply(
        root,
        &pkg("deepstream_extensions"),
        &url("https://github.com/example/pp.git"),
        "v1.1.0",
        &ver("1.1.0"),
        &sha("1111111111111111111111111111111111111111"),
        None,
    )
    .unwrap();
    assert_eq!(
        path,
        std::path::Path::new("vendor_recipes/deepstream-extensions/recipe.yaml")
    );
    let out =
        std::fs::read_to_string(root.join("vendor_recipes/deepstream-extensions/recipe.yaml"))
            .unwrap();
    assert!(out.contains("version: 1.1.0"));
    assert!(out.contains("rev: 1111111111111111111111111111111111111111"));
    assert!(out.contains("number: 0"));
}

#[test]
fn route_picks_pixi_native_over_rosdistro_when_both_list_the_package() {
    // A package that got migrated keeps its stale rosdistro block; the
    // pixi-native entry wins.
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "pixi_native_packages.yaml",
        "packages:\n  - name: both\n    url: https://github.com/example/both\n    rev: 0000000000000000000000000000000000000000\n",
    );
    write(
        root,
        "rosdistro_additional_recipes.yaml",
        "both:\n  url: https://github.com/example/both.git\n  tag: 0.1.0\n  version: 0.1.0\n",
    );
    let target = route(
        root,
        &pkg("both"),
        &url("https://github.com/example/both.git"),
        "v0.2.0",
        &ver("0.2.0"),
        &sha("1111111111111111111111111111111111111111"),
        PixiEntryOpts {
            subdir: None,
            lfs: false,
        },
    )
    .unwrap();
    assert!(matches!(target, ReleaseTarget::PixiNative { .. }));
    assert_eq!(
        target.rel_path(),
        std::path::Path::new("pixi_native_packages.yaml")
    );
    assert!(
        std::fs::read_to_string(root.join("rosdistro_additional_recipes.yaml"))
            .unwrap()
            .contains("tag: 0.1.0")
    );
}

#[test]
fn route_carries_only_the_facts_its_arm_records() {
    // The vendored recipe keeps its own source.git, so no url or tag reaches
    // the target; only the version and the sha it pins do.
    let td = tempfile::TempDir::new().unwrap();
    let root = td.path();
    write(
        root,
        "vendor_recipes/vend/recipe.yaml",
        "package:\n  name: vend\n  version: 1.0.0\n\nsource:\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 0\n",
    );
    let target = route(
        root,
        &pkg("vend"),
        &url("https://github.com/example/vend.git"),
        "v1.1.0",
        &ver("1.1.0"),
        &sha("1111111111111111111111111111111111111111"),
        PixiEntryOpts {
            subdir: Some("packages/vend"),
            lfs: false,
        },
    )
    .unwrap();
    assert_eq!(
        target,
        ReleaseTarget::Vendored {
            recipe_rel: std::path::PathBuf::from("vendor_recipes/vend/recipe.yaml"),
            version: ver("1.1.0"),
            sha: sha("1111111111111111111111111111111111111111"),
        }
    );
}
