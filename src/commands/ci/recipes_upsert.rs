use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::yaml_block::{self, item_bounds};

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

fn render(entry: &Entry, nl: &str) -> String {
    format!(
        "{name}:{nl}  url: {url}{nl}  tag: {tag}{nl}  version: {version}{nl}",
        name = entry.package,
        url = entry.url,
        tag = entry.tag,
        version = entry.version,
    )
}

fn upsert_text(body: &str, entry: &Entry) -> Result<String> {
    let nl = yaml_block::line_ending(body);

    // The package's block is its top-level `<name>:` key line plus everything
    // indented under it — see `yaml_block::section_bounds`, which is also what
    // decides whether the entry exists at all.
    if let Some(block) = yaml_block::section_bounds(body, entry.package) {
        let mut out = String::with_capacity(body.len());
        out.push_str(block.before(body));
        out.push_str(&render(entry, nl));
        out.push_str(block.after(body));
        return Ok(out);
    }

    // Append at EOF, separating with a blank line if the file is non-empty
    // and doesn't already end with one.
    let mut out = body.to_string();
    let blank_tail = format!("{nl}{nl}");
    if !out.is_empty() && !out.ends_with(&blank_tail) {
        if !out.ends_with(nl) {
            out.push_str(nl);
        }
        out.push_str(nl);
    }
    out.push_str(&render(entry, nl));
    Ok(out)
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
/// - The file is rebuilt line by line, so it comes back with the terminator
///   [`yaml_block::line_ending`] reports — see that module's line-ending note.
pub(crate) fn mutate_vendored_recipe(
    text: &str,
    version: &str,
    rev: &str,
) -> anyhow::Result<String> {
    let mut out: Vec<String> = Vec::new();
    let mut old_version: Option<String> = None;
    let mut old_rev: Option<String> = None;
    let mut number_idx: Option<usize> = None;

    for (section, line) in yaml_block::with_sections(text) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
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

    let nl = yaml_block::line_ending(text);
    let mut result = out.join(nl);
    if text.ends_with('\n') {
        result.push_str(nl);
    }
    Ok(result)
}

/// Mutate `pixi_native_packages.yaml` in place.
///
/// File shape: top-level `packages:` followed by `- name: <name>` items at
/// column-2 indent (two-space indent for the dash, and sub-keys at column 4).
///
/// Behavior:
/// - If an item with the given `name` exists: update `url:`, `rev:`,
///   optionally `subdir:` (insert if absent), delete `ref:` if present.
/// - If absent: append a new item at the end of the file with the same
///   indentation conventions.
/// - The file is rebuilt line by line, so it comes back with the terminator
///   [`yaml_block::line_ending`] reports — see that module's line-ending note.
pub(crate) fn mutate_pixi_entry(
    text: &str,
    name: &str,
    url: &str,
    rev: &str,
    subdir: Option<&str>,
) -> anyhow::Result<String> {
    let lines: Vec<&str> = text.lines().collect();

    let result = if let Some(item) = item_bounds(&lines, name) {
        let sub_indent = item.sub_indent();
        let mut out: Vec<String> = item
            .through_header(&lines)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut url_seen = false;
        let mut rev_seen = false;
        let mut subdir_seen = false;
        for line in item.body(&lines) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("ref:") {
                // drop
                continue;
            }
            if trimmed.starts_with("url:") {
                url_seen = true;
                out.push(format!("{}url: {}", " ".repeat(sub_indent), url));
                continue;
            }
            if trimmed.starts_with("rev:") {
                rev_seen = true;
                out.push(format!("{}rev: {}", " ".repeat(sub_indent), rev));
                continue;
            }
            if trimmed.starts_with("subdir:") {
                if let Some(s) = subdir {
                    subdir_seen = true;
                    out.push(format!("{}subdir: {}", " ".repeat(sub_indent), s));
                } // else: drop the existing subdir line — caller didn't pass one
                continue;
            }
            out.push(line.to_string());
        }
        if !url_seen {
            out.push(format!("{}url: {}", " ".repeat(sub_indent), url));
        }
        if !rev_seen {
            out.push(format!("{}rev: {}", " ".repeat(sub_indent), rev));
        }
        if !subdir_seen && let Some(s) = subdir {
            out.push(format!("{}subdir: {}", " ".repeat(sub_indent), s));
        }
        // The blank lines between this item and the next, then the rest of
        // the file, pass through untouched.
        for line in item.trailing(&lines) {
            out.push(line.to_string());
        }
        out
    } else {
        // Append a new entry at end of file.
        let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        // Ensure separation from previous content.
        if out.last().map(|s| !s.is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        out.push(format!("  - name: {name}"));
        out.push(format!("    url: {url}"));
        out.push(format!("    rev: {rev}"));
        if let Some(s) = subdir {
            out.push(format!("    subdir: {s}"));
        }
        out
    };

    let nl = yaml_block::line_ending(text);
    let mut result_str = result.join(nl);
    if text.ends_with('\n') {
        result_str.push_str(nl);
    }
    Ok(result_str)
}

/// True if `rosdistro_additional_recipes.yaml` text has a top-level
/// `<package>:` key — i.e. exactly the block `upsert_text` would replace.
fn rosdistro_has_entry(text: &str, package: &str) -> bool {
    yaml_block::section_bounds(text, package).is_some()
}

/// The ref a recipe pinned a package to before this release. `Rev` is an
/// immutable commit sha (vendored `source.rev`, pixi-native `rev:`); `Tag` is a
/// mutable tag/branch (rosdistro `tag:`, pixi-native `ref:`). When both are
/// available a `Rev` is preferred for diffing because it can't be moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OldRef {
    Rev(String),
    Tag(String),
}

impl OldRef {
    pub fn value(&self) -> &str {
        match self {
            OldRef::Rev(s) | OldRef::Tag(s) => s,
        }
    }

    pub fn is_rev(&self) -> bool {
        matches!(self, OldRef::Rev(_))
    }
}

/// The outcome of applying one package's release: which repo-relative file
/// changed (for staging) and the ref the package was pinned to *before* this
/// release (for building a source-repo diff link). `old_ref` is `None` for a
/// brand-new package that had no prior pin.
#[derive(Debug)]
pub(crate) struct Applied {
    pub path: PathBuf,
    pub old_ref: Option<OldRef>,
}

/// Locate a hand-authored vendored recipe for `package`, tolerating the
/// underscore→hyphen convention gap (ROS/tag names use `_`; conda recipe dirs
/// use `-`). Returns the repo-relative path to the first existing recipe,
/// trying `package` verbatim then its hyphenated form. `None` if neither exists.
pub(crate) fn vendored_recipe_path(recipes_root: &Path, package: &str) -> Option<PathBuf> {
    let mut candidates = vec![package.to_string()];
    let hyphenated = package.replace('_', "-");
    if hyphenated != package {
        candidates.push(hyphenated);
    }
    candidates.into_iter().find_map(|name| {
        let rel = Path::new("vendor_recipes").join(&name).join("recipe.yaml");
        recipes_root.join(&rel).exists().then_some(rel)
    })
}

/// Apply a release for one package to the cloned recipes repo.
///
/// Routing (first match wins):
///  1. `vendor_recipes/<package>/recipe.yaml` exists -> patch it (version + rev).
///  2. package already has an entry in `pixi_native_packages.yaml` -> update there.
///  3. package already has an entry in `rosdistro_additional_recipes.yaml` -> update there.
///  4. otherwise (brand-new) -> default to `pixi_native_packages.yaml`.
///
/// For the vendored path, `url`/`tag`/`subdir` are unused (the recipe keeps its
/// own source.git; only version + sha change). For the rosdistro path, `sha`/
/// `subdir` are unused. For the pixi-native path, `tag` is unused (`sha` -> rev).
pub(crate) fn apply_release(
    recipes_root: &Path,
    package: &str,
    url: &str,
    tag: &str,
    version: &str,
    sha: &str,
    subdir: Option<&str>,
) -> anyhow::Result<Applied> {
    // 1. Vendored (name-convention tolerant: `_` ROS name -> `-` recipe dir).
    if let Some(vendored_rel) = vendored_recipe_path(recipes_root, package) {
        let vendored_abs = recipes_root.join(&vendored_rel);
        let text = std::fs::read_to_string(&vendored_abs)
            .with_context(|| format!("reading {}", vendored_abs.display()))?;
        let old_ref = yaml_block::field_of(&text, "source", "rev")
            .map(str::to_string)
            .map(OldRef::Rev);
        let updated = mutate_vendored_recipe(&text, version, sha)?;
        std::fs::write(&vendored_abs, updated)
            .with_context(|| format!("writing {}", vendored_abs.display()))?;
        return Ok(Applied {
            path: vendored_rel,
            old_ref,
        });
    }

    let pixi_native_rel = PathBuf::from(crate::consts::PIXI_NATIVE_PACKAGES_YAML);
    let rosdistro_rel = PathBuf::from(crate::consts::ROSDISTRO_RECIPES_YAML);
    let pixi_native_abs = recipes_root.join(&pixi_native_rel);
    let rosdistro_abs = recipes_root.join(&rosdistro_rel);

    let pixi_native_text = if pixi_native_abs.exists() {
        Some(
            std::fs::read_to_string(&pixi_native_abs)
                .with_context(|| format!("reading {}", pixi_native_abs.display()))?,
        )
    } else {
        None
    };
    let rosdistro_text = if rosdistro_abs.exists() {
        Some(
            std::fs::read_to_string(&rosdistro_abs)
                .with_context(|| format!("reading {}", rosdistro_abs.display()))?,
        )
    } else {
        None
    };
    let in_pixi_native = match pixi_native_text.as_deref() {
        Some(t) => crate::types::PixiNativeManifest::has_entry(t, package)
            .with_context(|| format!("parsing {}", pixi_native_abs.display()))?,
        None => false,
    };
    let in_rosdistro = rosdistro_text
        .as_deref()
        .is_some_and(|t| rosdistro_has_entry(t, package));

    // Arm 3: existing rosdistro entry (and not already pixi-native) -> update there.
    if in_rosdistro && !in_pixi_native {
        let old_ref = rosdistro_text
            .as_deref()
            .and_then(|t| yaml_block::field_of(t, package, "tag"))
            .map(str::to_string)
            .map(OldRef::Tag);
        upsert(
            &rosdistro_abs,
            &Entry {
                package,
                url,
                tag,
                version,
            },
        )?;
        return Ok(Applied {
            path: rosdistro_rel,
            old_ref,
        });
    }

    // Arms 2 & 4: existing pixi-native entry, or brand-new package -> pixi-native.
    let text = pixi_native_text.ok_or_else(|| {
        anyhow::anyhow!(
            "{} not found in recipes repo; cannot add pixi-native entry for {package}",
            pixi_native_abs.display()
        )
    })?;
    let old_ref = pixi_entry_rev(&text, package);
    let updated = mutate_pixi_entry(&text, package, url, sha, subdir)?;
    std::fs::write(&pixi_native_abs, updated)
        .with_context(|| format!("writing {}", pixi_native_abs.display()))?;
    Ok(Applied {
        path: pixi_native_rel,
        old_ref,
    })
}

/// The pin a `pixi_native_packages.yaml` entry currently carries: `rev:` as an
/// immutable [`OldRef::Rev`], `ref:` as a mutable [`OldRef::Tag`]. `None` if the
/// entry or field is absent.
fn pixi_entry_rev(text: &str, name: &str) -> Option<OldRef> {
    let lines: Vec<&str> = text.lines().collect();
    let item = item_bounds(&lines, name)?;
    item.body(&lines).iter().find_map(|l| {
        let t = l.trim_start();
        if let Some(v) = t.strip_prefix("rev:") {
            return Some(OldRef::Rev(v.trim().to_string()));
        }
        t.strip_prefix("ref:")
            .map(|v| OldRef::Tag(v.trim().to_string()))
    })
}

#[cfg(test)]
#[path = "recipes_upsert_tests.rs"]
mod tests;
