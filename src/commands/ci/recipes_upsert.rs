use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;

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
}
