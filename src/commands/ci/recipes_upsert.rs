use color_eyre::eyre::{Result, WrapErr};
use std::path::{Path, PathBuf};

use super::yaml_block::{self, ItemBounds, indent_of, item_bounds};
use crate::types::{GithubRepoUrl, PackageName, Sha40, Version};

/// An entry in `rosdistro_additional_recipes.yaml`, emitted as url, tag, version.
pub struct Entry<'a> {
    pub package: &'a PackageName,
    pub url: &'a GithubRepoUrl,
    pub tag: &'a str,
    pub version: &'a Version,
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

    std::fs::write(recipes_yaml, upsert_text(&body, entry))
        .with_context(|| format!("writing {}", recipes_yaml.display()))?;
    Ok(())
}

fn render(entry: &Entry, nl: &str) -> String {
    format!(
        "{name}:{nl}  url: {url}{nl}  tag: {tag}{nl}  version: {version}{nl}",
        name = entry.package,
        url = entry.url.git_url(),
        tag = entry.tag,
        version = entry.version,
    )
}

fn upsert_text(body: &str, entry: &Entry) -> String {
    let nl = yaml_block::line_ending(body);

    if let Some(block) = yaml_block::section_bounds(body, entry.package.as_str()) {
        let mut out = String::with_capacity(body.len());
        out.push_str(block.before());
        out.push_str(&render(entry, nl));
        out.push_str(block.after());
        return out;
    }

    let mut out = body.to_string();
    let blank_tail = format!("{nl}{nl}");
    if !out.is_empty() && !out.ends_with(&blank_tail) {
        if !out.ends_with(nl) {
            out.push_str(nl);
        }
        out.push_str(nl);
    }
    out.push_str(&render(entry, nl));
    out
}

/// Mutate a hand-authored `vendor_recipes/<pkg>/recipe.yaml`, returning the
/// new content:
/// - Replaces `package.version` and `source.rev`.
/// - Resets `build.number` to 0 only when the version actually changed —
///   manual rebuild bumps stay increment-only.
/// - Returns the input unchanged when version and rev already match.
/// - Errors if any of the three fields is missing — never a silent no-op.
/// - Everything else (comments, deps, formatting) passes through untouched.
pub(crate) fn mutate_vendored_recipe(
    text: &str,
    version: &Version,
    rev: &Sha40,
) -> color_eyre::eyre::Result<String> {
    let mut out: Vec<String> = Vec::new();
    let mut old_version: Option<String> = None;
    let mut old_rev: Option<String> = None;
    let mut number_idx: Option<usize> = None;

    for (section, line) in yaml_block::with_sections(text) {
        let trimmed = line.trim_start();
        let indent = indent_of(line);
        let replacement = match section {
            Some("package") if indent > 0 => trimmed.strip_prefix("version:").map(|old| {
                old_version = Some(old.trim().to_string());
                format!("{}version: {}", " ".repeat(indent), version)
            }),
            Some("source") if indent > 0 => trimmed.strip_prefix("rev:").map(|old| {
                old_rev = Some(old.trim().to_string());
                format!("{}rev: {}", " ".repeat(indent), rev)
            }),
            Some("build") if indent > 0 && trimmed.starts_with("number:") => {
                number_idx = Some(out.len());
                None
            }
            _ => None,
        };
        out.push(replacement.unwrap_or_else(|| line.to_string()));
    }

    let old_version = old_version
        .ok_or_else(|| color_eyre::eyre::eyre!("package.version not found in recipe"))?;
    if old_version.contains("${{") {
        color_eyre::eyre::bail!(
            "package.version is templated ({old_version}); refusing to overwrite"
        );
    }
    let old_rev =
        old_rev.ok_or_else(|| color_eyre::eyre::eyre!("source.rev not found in recipe"))?;
    let number_idx =
        number_idx.ok_or_else(|| color_eyre::eyre::eyre!("build.number not found in recipe"))?;

    let version = version.to_string();
    let rev = rev.as_str();
    if old_version == version && old_rev == rev {
        return Ok(text.to_string());
    }
    if old_version != version
        && let Some(number) = out.get_mut(number_idx)
    {
        *number = format!("{}number: 0", " ".repeat(indent_of(number)));
    }

    let nl = yaml_block::line_ending(text);
    let mut result = out.join(nl);
    if text.ends_with('\n') {
        result.push_str(nl);
    }
    Ok(result)
}

/// Rewrite one `- name:` item of `pixi_native_packages.yaml`, appending it if
/// absent, and return the new text.
///
/// File shape: top-level `packages:` followed by `- name: <name>` items at
/// column-2 indent (two-space indent for the dash, and sub-keys at column 4).
///
/// `subdir` and `lfs` are authoritative facts about the package, not overlays:
/// `None`/`false` remove an existing `subdir:`/`lfs:` line.
pub(crate) fn mutate_pixi_entry(
    text: &str,
    name: &PackageName,
    url: &GithubRepoUrl,
    rev: &Sha40,
    subdir: Option<&str>,
    lfs: bool,
) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let url = url.git_url();
    let fields = PixiFields {
        url: &url,
        rev: rev.as_str(),
        subdir,
        lfs,
    };

    let result = item_bounds(&lines, name.as_str()).map_or_else(
        || appended_pixi_item(&lines, name, &fields),
        |item| updated_pixi_item(&item, &fields),
    );

    let nl = yaml_block::line_ending(text);
    let mut result_str = result.join(nl);
    if text.ends_with('\n') {
        result_str.push_str(nl);
    }
    result_str
}

struct PixiFields<'a> {
    url: &'a str,
    rev: &'a str,
    subdir: Option<&'a str>,
    lfs: bool,
}

fn updated_pixi_item(item: &ItemBounds, fields: &PixiFields) -> Vec<String> {
    let sub_indent = item.sub_indent();
    let mut out: Vec<String> = item
        .through_header()
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    let mut url_seen = false;
    let mut rev_seen = false;
    let mut subdir_seen = false;
    let mut lfs_seen = false;
    for line in item.body() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("ref:") {
            continue;
        }
        if trimmed.starts_with("url:") {
            url_seen = true;
            out.push(format!("{}url: {}", " ".repeat(sub_indent), fields.url));
            continue;
        }
        if trimmed.starts_with("rev:") {
            rev_seen = true;
            out.push(format!("{}rev: {}", " ".repeat(sub_indent), fields.rev));
            continue;
        }
        if trimmed.starts_with("subdir:") {
            if let Some(s) = fields.subdir {
                subdir_seen = true;
                out.push(format!("{}subdir: {}", " ".repeat(sub_indent), s));
            }
            continue;
        }
        if trimmed.starts_with("lfs:") {
            if fields.lfs {
                lfs_seen = true;
                out.push(format!("{}lfs: true", " ".repeat(sub_indent)));
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !url_seen {
        out.push(format!("{}url: {}", " ".repeat(sub_indent), fields.url));
    }
    if !rev_seen {
        out.push(format!("{}rev: {}", " ".repeat(sub_indent), fields.rev));
    }
    if !subdir_seen && let Some(s) = fields.subdir {
        out.push(format!("{}subdir: {}", " ".repeat(sub_indent), s));
    }
    if !lfs_seen && fields.lfs {
        out.push(format!("{}lfs: true", " ".repeat(sub_indent)));
    }
    for line in item.trailing() {
        out.push(line.to_string());
    }
    out
}

fn appended_pixi_item(lines: &[&str], name: &PackageName, fields: &PixiFields) -> Vec<String> {
    let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
    if out.last().is_some_and(|s| !s.is_empty()) {
        out.push(String::new());
    }
    out.push(format!("  - name: {name}"));
    out.push(format!("    url: {}", fields.url));
    out.push(format!("    rev: {}", fields.rev));
    if let Some(s) = fields.subdir {
        out.push(format!("    subdir: {s}"));
    }
    if fields.lfs {
        out.push("    lfs: true".to_string());
    }
    out
}

/// True if `rosdistro_additional_recipes.yaml` text has a top-level
/// `<package>:` key — i.e. exactly the block `upsert_text` would replace.
fn rosdistro_has_entry(text: &str, package: &PackageName) -> bool {
    yaml_block::section_bounds(text, package.as_str()).is_some()
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
            Self::Rev(s) | Self::Tag(s) => s,
        }
    }

    pub const fn is_rev(&self) -> bool {
        matches!(self, Self::Rev(_))
    }
}

/// Locate a hand-authored vendored recipe for `package`, tolerating the
/// underscore→hyphen convention gap (ROS/tag names use `_`; conda recipe dirs
/// use `-`). Returns the repo-relative path to the first existing recipe,
/// trying `package` verbatim then its hyphenated form. `None` if neither exists.
pub(crate) fn vendored_recipe_path(recipes_root: &Path, package: &PackageName) -> Option<PathBuf> {
    let package = package.as_str();
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

/// Where one package's release lands in the recipes repo. Each arm holds
/// exactly the fields its file format records: a vendored recipe keeps its
/// own `source.git`, rosdistro pins a mutable tag, pixi-native pins a sha.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReleaseTarget {
    /// A hand-authored `vendor_recipes/<dir>/recipe.yaml`, patched in place.
    /// `recipe_rel` is resolved during routing (the dir may be the hyphenated
    /// spelling of the package name), so it is carried rather than re-derived.
    Vendored {
        recipe_rel: PathBuf,
        version: Version,
        sha: Sha40,
    },
    /// An entry in `rosdistro_additional_recipes.yaml`, which pins a tag.
    Rosdistro {
        package: PackageName,
        url: GithubRepoUrl,
        tag: String,
        version: Version,
    },
    /// An entry in `pixi_native_packages.yaml`, which pins a rev.
    PixiNative {
        package: PackageName,
        url: GithubRepoUrl,
        sha: Sha40,
        subdir: Option<String>,
        lfs: bool,
    },
}

impl ReleaseTarget {
    /// The repo-relative file this target writes.
    pub fn rel_path(&self) -> PathBuf {
        match self {
            Self::Vendored { recipe_rel, .. } => recipe_rel.clone(),
            Self::Rosdistro { .. } => PathBuf::from(crate::consts::ROSDISTRO_RECIPES_YAML),
            Self::PixiNative { .. } => PathBuf::from(crate::consts::PIXI_NATIVE_PACKAGES_YAML),
        }
    }
}

/// The facts only a `pixi_native_packages.yaml` entry records.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PixiEntryOpts<'a> {
    pub subdir: Option<&'a str>,
    pub lfs: bool,
}

/// Decide where a release for `package` lands, reading the cloned recipes repo.
///
/// Routing (first match wins):
///  1. `vendor_recipes/<package>/recipe.yaml` exists -> patch it (version + rev).
///  2. package already has an entry in `pixi_native_packages.yaml` -> update there.
///  3. package already has an entry in `rosdistro_additional_recipes.yaml` -> update there.
///  4. otherwise (brand-new) -> default to `pixi_native_packages.yaml`.
///
/// Pure decision plus reads: nothing here writes.
pub(crate) fn route(
    recipes_root: &Path,
    package: &PackageName,
    url: &GithubRepoUrl,
    tag: &str,
    version: &Version,
    sha: &Sha40,
    pixi: PixiEntryOpts<'_>,
) -> color_eyre::eyre::Result<ReleaseTarget> {
    if let Some(recipe_rel) = vendored_recipe_path(recipes_root, package) {
        return Ok(ReleaseTarget::Vendored {
            recipe_rel,
            version: version.clone(),
            sha: sha.clone(),
        });
    }

    let pixi_native_abs = recipes_root.join(crate::consts::PIXI_NATIVE_PACKAGES_YAML);
    let rosdistro_abs = recipes_root.join(crate::consts::ROSDISTRO_RECIPES_YAML);

    let pixi_native_text = read_if_exists(&pixi_native_abs)?;
    let rosdistro_text = read_if_exists(&rosdistro_abs)?;
    let in_pixi_native = match pixi_native_text.as_deref() {
        Some(t) => crate::types::PixiNativeManifest::has_entry(t, package)
            .with_context(|| format!("parsing {}", pixi_native_abs.display()))?,
        None => false,
    };
    let in_rosdistro = rosdistro_text
        .as_deref()
        .is_some_and(|t| rosdistro_has_entry(t, package));

    if in_rosdistro && !in_pixi_native {
        return Ok(ReleaseTarget::Rosdistro {
            package: package.clone(),
            url: url.clone(),
            tag: tag.to_string(),
            version: version.clone(),
        });
    }

    // The file has to exist to append to; fail here rather than in the write stage.
    color_eyre::eyre::ensure!(
        pixi_native_text.is_some(),
        "{} not found in recipes repo; cannot add pixi-native entry for {package}",
        pixi_native_abs.display()
    );
    Ok(ReleaseTarget::PixiNative {
        package: package.clone(),
        url: url.clone(),
        sha: sha.clone(),
        subdir: pixi.subdir.map(str::to_string),
        lfs: pixi.lfs,
    })
}

fn read_if_exists(path: &Path) -> color_eyre::eyre::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .with_context(|| format!("reading {}", path.display()))
}

/// Write a routed release into the cloned recipes repo.
///
/// Returns the ref the package was pinned to *before* this release (for
/// building a source-repo diff link), or `None` for a brand-new package that
/// had no prior pin. The file written is [`ReleaseTarget::rel_path`].
pub(crate) fn apply(
    recipes_root: &Path,
    target: &ReleaseTarget,
) -> color_eyre::eyre::Result<Option<OldRef>> {
    let abs = recipes_root.join(target.rel_path());
    match target {
        ReleaseTarget::Vendored {
            version,
            sha,
            recipe_rel: _,
        } => {
            let text = std::fs::read_to_string(&abs)
                .with_context(|| format!("reading {}", abs.display()))?;
            let old_ref = yaml_block::field_of(&text, "source", "rev")
                .map(str::to_string)
                .map(OldRef::Rev);
            let updated = mutate_vendored_recipe(&text, version, sha)?;
            std::fs::write(&abs, updated).with_context(|| format!("writing {}", abs.display()))?;
            Ok(old_ref)
        }
        ReleaseTarget::Rosdistro {
            package,
            url,
            tag,
            version,
        } => {
            let old_ref = read_if_exists(&abs)?
                .as_deref()
                .and_then(|t| yaml_block::field_of(t, package.as_str(), "tag"))
                .map(str::to_string)
                .map(OldRef::Tag);
            upsert(
                &abs,
                &Entry {
                    package,
                    url,
                    tag,
                    version,
                },
            )?;
            Ok(old_ref)
        }
        ReleaseTarget::PixiNative {
            package,
            url,
            sha,
            subdir,
            lfs,
        } => {
            let text = std::fs::read_to_string(&abs)
                .with_context(|| format!("reading {}", abs.display()))?;
            let old_ref = pixi_entry_rev(&text, package);
            let updated = mutate_pixi_entry(&text, package, url, sha, subdir.as_deref(), *lfs);
            std::fs::write(&abs, updated).with_context(|| format!("writing {}", abs.display()))?;
            Ok(old_ref)
        }
    }
}

/// The pin a `pixi_native_packages.yaml` entry currently carries: `rev:` as an
/// immutable [`OldRef::Rev`], `ref:` as a mutable [`OldRef::Tag`]. `None` if the
/// entry or field is absent.
fn pixi_entry_rev(text: &str, name: &PackageName) -> Option<OldRef> {
    let lines: Vec<&str> = text.lines().collect();
    let item = item_bounds(&lines, name.as_str())?;
    item.body().iter().find_map(|l| {
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
