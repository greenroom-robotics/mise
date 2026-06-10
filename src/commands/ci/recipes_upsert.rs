use anyhow::{Context, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

/// An entry in `rosdistro_additional_recipes.yaml`. Fields are emitted in this
/// fixed order: url, tag, version. Matches the existing format in ros-recipes.
pub struct Entry<'a> {
    pub package: &'a str,
    pub url: &'a str,
    pub tag: &'a str,
    pub version: &'a str,
}

/// Idempotently upsert `entry` into the recipes YAML file. Comments and other
/// entries are preserved verbatim. If the file doesn't exist, it's created
/// with just this entry.
pub fn upsert(recipes_yaml: &Path, entry: &Entry) -> Result<()> {
    let body = if recipes_yaml.exists() {
        std::fs::read_to_string(recipes_yaml)
            .with_context(|| format!("reading {}", recipes_yaml.display()))?
    } else {
        String::new()
    };

    let new_body = upsert_text(&body, entry)?;
    std::fs::write(recipes_yaml, new_body)
        .with_context(|| format!("writing {}", recipes_yaml.display()))?;
    Ok(())
}

fn render(entry: &Entry) -> String {
    format!(
        "{name}:\n  url: {url}\n  tag: {tag}\n  version: {version}\n",
        name = entry.package,
        url = entry.url,
        tag = entry.tag,
        version = entry.version,
    )
}

fn upsert_text(body: &str, entry: &Entry) -> Result<String> {
    // Match the package's top-level key line: `name:` at column 0 (no leading
    // whitespace), with the trailing colon. The block extends through every
    // following line that starts with whitespace or is blank, until the next
    // non-indented non-comment line (or EOF).
    let key_pattern = format!(r"(?m)^{}:[ \t]*\r?\n", regex::escape(entry.package));
    let key_re = Regex::new(&key_pattern)?;

    if let Some(m) = key_re.find(body) {
        // Find the end of the block.
        let start = m.start();
        let after_key_line = m.end();
        let tail = &body[after_key_line..];
        let mut block_end_offset = tail.len();
        for (line_start, line) in line_offsets(tail) {
            // The first non-indented, non-blank line ends the block.
            let is_indented = line.starts_with(' ') || line.starts_with('\t');
            let is_blank = line.trim().is_empty();
            if !is_indented && !is_blank {
                block_end_offset = line_start;
                break;
            }
        }
        let block_end = after_key_line + block_end_offset;
        let mut out = String::with_capacity(body.len());
        out.push_str(&body[..start]);
        out.push_str(&render(entry));
        out.push_str(&body[block_end..]);
        return Ok(out);
    }

    // Append at EOF, separating with a blank line if the file is non-empty
    // and doesn't already end with one.
    let mut out = body.to_string();
    if !out.is_empty() && !out.ends_with("\n\n") {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&render(entry));
    Ok(out)
}

fn line_offsets(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;
    s.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line.trim_end_matches('\n').trim_end_matches('\r'))
    })
}

/// Mutate a hand-authored `vendor_recipes/<pkg>/recipe.yaml` in place. Returns
/// the new content.
///
/// Behavior:
/// - Replaces `version:` inside the top-level `package:` block.
/// - Replaces `rev:` inside the top-level `source:` block.
/// - Resets `number:` inside the top-level `build:` block to 0, but only when
///   the version actually changed — manual rebuild bumps stay increment-only.
/// - Returns the input unchanged when version and rev already match, so
///   `open-pr` stages nothing and exits as a no-op.
/// - Errors if any of the three fields is missing — never a silent no-op.
/// - Every other line (comments, deps, formatting) passes through untouched.
pub(crate) fn mutate_vendored_recipe(
    text: &str,
    version: &str,
    rev: &str,
) -> anyhow::Result<String> {
    let mut section: Option<&str> = None;
    let mut out: Vec<String> = Vec::new();
    let mut old_version: Option<String> = None;
    let mut old_rev: Option<String> = None;
    let mut number_idx: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // `package:` → Some("package"); `key: value` → None (no bare-key suffix).
            section = trimmed.strip_suffix(':');
        }
        let replacement = match section {
            Some("package") if indent > 0 && trimmed.starts_with("version:") => {
                old_version = Some(trimmed["version:".len()..].trim().to_string());
                Some(format!("{}version: {}", " ".repeat(indent), version))
            }
            Some("source") if indent > 0 && trimmed.starts_with("rev:") => {
                old_rev = Some(trimmed["rev:".len()..].trim().to_string());
                Some(format!("{}rev: {}", " ".repeat(indent), rev))
            }
            Some("build") if indent > 0 && trimmed.starts_with("number:") => {
                number_idx = Some(out.len());
                None
            }
            _ => None,
        };
        out.push(replacement.unwrap_or_else(|| line.to_string()));
    }

    let old_version =
        old_version.ok_or_else(|| anyhow::anyhow!("package.version not found in recipe"))?;
    if old_version.contains("${{") {
        anyhow::bail!("package.version is templated ({old_version}); refusing to overwrite");
    }
    let old_rev = old_rev.ok_or_else(|| anyhow::anyhow!("source.rev not found in recipe"))?;
    let number_idx =
        number_idx.ok_or_else(|| anyhow::anyhow!("build.number not found in recipe"))?;

    if old_version == version && old_rev == rev {
        return Ok(text.to_string());
    }
    if old_version != version {
        let indent = out[number_idx].len() - out[number_idx].trim_start().len();
        out[number_idx] = format!("{}number: 0", " ".repeat(indent));
    }

    let mut result = out.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Apply a release for one package to the cloned recipes repo.
///
/// If `vendor_recipes/<package>/recipe.yaml` exists, patch it in place
/// (version + source.rev, resetting build.number on a version change) via
/// `mutate_vendored_recipe`. Otherwise upsert the package's entry into
/// `rosdistro_additional_recipes.yaml`. Returns the repo-relative path of the
/// file that changed, for staging.
///
/// Note: for the vendored path, `url` and `tag` are unused — the recipe keeps
/// its own `source.git` url; only `version` and `sha` (as `source.rev`) change.
pub(crate) fn apply_release(
    recipes_root: &Path,
    package: &str,
    url: &str,
    tag: &str,
    version: &str,
    sha: &str,
) -> anyhow::Result<PathBuf> {
    let vendored_rel = Path::new("vendor_recipes")
        .join(package)
        .join("recipe.yaml");
    let vendored_abs = recipes_root.join(&vendored_rel);
    if vendored_abs.exists() {
        let text = std::fs::read_to_string(&vendored_abs)
            .with_context(|| format!("reading {}", vendored_abs.display()))?;
        let updated = mutate_vendored_recipe(&text, version, sha)?;
        std::fs::write(&vendored_abs, updated)
            .with_context(|| format!("writing {}", vendored_abs.display()))?;
        Ok(vendored_rel)
    } else {
        let recipes_yaml_rel = PathBuf::from("rosdistro_additional_recipes.yaml");
        upsert(
            &recipes_root.join(&recipes_yaml_rel),
            &Entry {
                package,
                url,
                tag,
                version,
            },
        )?;
        Ok(recipes_yaml_rel)
    }
}

#[cfg(test)]
mod tests {
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
        let existing =
            "bar:\n  url: https://example.invalid/bar.git\n  tag: 0.1.0\n  version: 0.1.0\n";
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
        let no_rev =
            VENDORED_FIXTURE.replace("  rev: 4bcfd421c52387b3f7872b23e60059e521176f35\n", "");
        let err =
            mutate_vendored_recipe(&no_rev, "1.3.0", "1111111111111111111111111111111111111111")
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
        let templated =
            VENDORED_FIXTURE.replace("  version: 1.2.3\n", "  version: ${{ some_var }}\n");
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
    fn apply_release_patches_vendored_recipe_when_present() {
        let td = tempfile::TempDir::new().unwrap();
        let root = td.path();
        let dir = root.join("vendor_recipes/is-core");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("recipe.yaml"),
            "package:\n  name: is-core\n  version: 1.0.0\n\nsource:\n  git: https://github.com/example/is-core.git\n  rev: 0000000000000000000000000000000000000000\n\nbuild:\n  number: 2\n",
        ).unwrap();
        let changed = apply_release(
            root,
            "is-core",
            "https://github.com/example/is-core.git",
            "v1.1.0",
            "1.1.0",
            "1111111111111111111111111111111111111111",
        )
        .unwrap();
        assert_eq!(
            changed,
            std::path::Path::new("vendor_recipes/is-core/recipe.yaml")
        );
        let out = std::fs::read_to_string(dir.join("recipe.yaml")).unwrap();
        assert!(out.contains("version: 1.1.0"));
        assert!(out.contains("rev: 1111111111111111111111111111111111111111"));
        assert!(out.contains("number: 0"));
        // rosdistro file must NOT have been created
        assert!(!root.join("rosdistro_additional_recipes.yaml").exists());
    }

    #[test]
    fn apply_release_upserts_rosdistro_when_not_vendored() {
        let td = tempfile::TempDir::new().unwrap();
        let root = td.path();
        let changed = apply_release(
            root,
            "foo_pkg",
            "https://github.com/example/foo_pkg.git",
            "v2.0.0",
            "2.0.0",
            "2222222222222222222222222222222222222222",
        )
        .unwrap();
        assert_eq!(
            changed,
            std::path::Path::new("rosdistro_additional_recipes.yaml")
        );
        let out = std::fs::read_to_string(root.join("rosdistro_additional_recipes.yaml")).unwrap();
        assert!(out.contains("foo_pkg:"));
        assert!(out.contains("tag: v2.0.0"));
        assert!(out.contains("version: 2.0.0"));
    }
}
