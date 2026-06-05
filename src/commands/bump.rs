use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Subcommand;
use serde::{Deserialize, Serialize};

use crate::types::Sha40;

#[derive(Subcommand, Debug)]
pub enum Bump {
    /// Update a recipe entry in rosdistro_additional_recipes.yaml.
    Recipe {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        /// Package key in the YAML (top-level map key).
        package: String,
        /// New `version:` value.
        version: String,
        /// New `rev:` value (40-char SHA). Also deletes any existing `tag:`/`branch:`.
        rev: String,
    },
    /// Update or insert a package entry in pixi_native_packages.yaml.
    Pixi {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        /// Entry `name:` to find or create.
        name: String,
        /// `url:` value.
        url: String,
        /// `rev:` value (40-char SHA). Deletes any existing `ref:`.
        rev: String,
        /// Optional `subdir:` value.
        #[arg(long)]
        subdir: Option<String>,
    },
    /// Read a dispatch payload JSON and dispatch to bump recipe or bump pixi.
    Route {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        payload: PathBuf,
    },
    /// Commit the recipe-yaml edits and open/update an auto-merge PR.
    OpenPr {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        payload: PathBuf,
    },
}

impl Bump {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Recipe {
                repo_root,
                package,
                version,
                rev,
            } => recipe(repo_root, package, version, rev),
            Self::Pixi {
                repo_root,
                name,
                url,
                rev,
                subdir,
            } => pixi(repo_root, name, url, rev, subdir),
            Self::Route { repo_root, payload } => route(repo_root, payload),
            Self::OpenPr { repo_root, payload } => open_pr(repo_root, payload),
        }
    }
}

use crate::repo::Repo;

fn recipe(
    repo_root: Option<PathBuf>,
    package: String,
    version: String,
    rev: String,
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let path = repo.root().join("rosdistro_additional_recipes.yaml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let updated = mutate_recipe_entry(&text, &package, &version, &rev)?;
    std::fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Mutate a top-level-keyed YAML in-place style. Returns the new content.
///
/// Behavior:
/// - The `package:` block must exist; otherwise returns an error.
/// - Within that block (until the next top-level key or EOF), delete any
///   `tag:` and `branch:` lines, replace any `version:` line, replace any
///   `rev:` line (or insert one after the first sub-key line if not present).
/// - All other lines (including comments) pass through.
pub(crate) fn mutate_recipe_entry(
    text: &str,
    package: &str,
    version: &str,
    rev: &str,
) -> anyhow::Result<String> {
    let header = format!("{package}:");
    let lines: Vec<&str> = text.lines().collect();

    let header_idx = lines
        .iter()
        .position(|l| l.trim_end() == header)
        .ok_or_else(|| anyhow::anyhow!("entry {package:?} not found"))?;

    // Block spans header_idx+1 .. block_end (exclusive). A line ends the
    // block when it starts at column 0 with non-whitespace and is not blank.
    let block_end = lines[header_idx + 1..]
        .iter()
        .position(|l| !l.is_empty() && !l.starts_with(' ') && !l.starts_with('\t'))
        .map(|p| header_idx + 1 + p)
        .unwrap_or(lines.len());

    // Trailing blank lines in the range belong to the gap *between* entries, not
    // to this entry. Walk back so the mutation doesn't absorb them.
    let mut block_actual_end = block_end;
    while block_actual_end > header_idx + 1 && lines[block_actual_end - 1].trim().is_empty() {
        block_actual_end -= 1;
    }

    let mut out: Vec<String> = lines[..=header_idx].iter().map(|s| s.to_string()).collect();
    let mut rev_seen = false;
    let mut version_seen = false;
    // The indent of the first non-blank sub-line is the block's indent.
    let block_indent = lines[header_idx + 1..block_actual_end]
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(2);

    for line in &lines[header_idx + 1..block_actual_end] {
        let trimmed = line.trim_start();
        if trimmed.starts_with("tag:") || trimmed.starts_with("branch:") {
            // drop
            continue;
        }
        if trimmed.starts_with("version:") {
            version_seen = true;
            out.push(format!("{}version: {}", " ".repeat(block_indent), version));
            continue;
        }
        if trimmed.starts_with("rev:") {
            rev_seen = true;
            out.push(format!("{}rev: {}", " ".repeat(block_indent), rev));
            continue;
        }
        out.push(line.to_string());
    }
    if !rev_seen {
        // Insert just after the header to keep rev visible near the top of the block.
        out.insert(
            header_idx + 1,
            format!("{}rev: {}", " ".repeat(block_indent), rev),
        );
    }
    if !version_seen {
        out.push(format!("{}version: {}", " ".repeat(block_indent), version));
    }

    // Preserve the original between-entry blank lines.
    for line in &lines[block_actual_end..block_end] {
        out.push(line.to_string());
    }
    for line in &lines[block_end..] {
        out.push(line.to_string());
    }

    let has_trailing_newline = text.ends_with('\n');
    let mut result = out.join("\n");
    if has_trailing_newline {
        result.push('\n');
    }
    Ok(result)
}

fn pixi(
    repo_root: Option<PathBuf>,
    name: String,
    url: String,
    rev: String,
    subdir: Option<String>,
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let path = repo.root().join("pixi_native_packages.yaml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let updated = mutate_pixi_entry(&text, &name, &url, &rev, subdir.as_deref())?;
    std::fs::write(&path, updated).with_context(|| format!("write {}", path.display()))?;
    Ok(())
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
pub(crate) fn mutate_pixi_entry(
    text: &str,
    name: &str,
    url: &str,
    rev: &str,
    subdir: Option<&str>,
) -> anyhow::Result<String> {
    let lines: Vec<&str> = text.lines().collect();

    // Find `- name: <name>` line.
    let header = format!("- name: {name}");
    let header_idx = lines.iter().position(|l| l.trim_start() == header);

    let result = if let Some(idx) = header_idx {
        // Determine the item's column-position. Sub-keys live at the same
        // start column as `name` (i.e., 2 chars in from the `-`).
        let item_indent = lines[idx].len() - lines[idx].trim_start().len();
        let sub_indent = item_indent + 2;
        // Block spans idx+1 .. block_end where block_end starts at a line
        // whose indent is <= item_indent and is not blank.
        let block_end = lines[idx + 1..]
            .iter()
            .position(|l| {
                if l.trim().is_empty() {
                    return false;
                }
                let li = l.len() - l.trim_start().len();
                li <= item_indent
            })
            .map(|p| idx + 1 + p)
            .unwrap_or(lines.len());

        // Trailing blank lines in the range belong to the gap *between* entries.
        // Walk back so the mutation doesn't absorb them into this item.
        let mut block_actual_end = block_end;
        while block_actual_end > idx + 1 && lines[block_actual_end - 1].trim().is_empty() {
            block_actual_end -= 1;
        }

        let mut out: Vec<String> = lines[..=idx].iter().map(|s| s.to_string()).collect();
        let mut url_seen = false;
        let mut rev_seen = false;
        let mut subdir_seen = false;
        for line in &lines[idx + 1..block_actual_end] {
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
        // Preserve original between-entry blank lines.
        for line in &lines[block_actual_end..block_end] {
            out.push(line.to_string());
        }
        for line in &lines[block_end..] {
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

    let has_trailing_newline = text.ends_with('\n');
    let mut result_str = result.join("\n");
    if has_trailing_newline {
        result_str.push('\n');
    }
    Ok(result_str)
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

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct DispatchPayload {
    pub package: String,
    pub version: String,
    pub source_repo: String,
    pub sha: Sha40,
    #[serde(default)]
    pub manifest_type: Option<ManifestType>,
    #[serde(default)]
    pub subdir: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestType {
    #[serde(rename = "pixi.toml")]
    PixiToml,
    #[serde(rename = "package.xml")]
    PackageXml,
}

impl DispatchPayload {
    pub fn manifest_type_or_default(&self) -> ManifestType {
        self.manifest_type.unwrap_or(ManifestType::PixiToml)
    }
    pub fn source_url(&self) -> String {
        format!("https://github.com/{}", self.source_repo)
    }
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read payload {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parse payload {}", path.display()))
    }
}

fn route(repo_root: Option<PathBuf>, payload: PathBuf) -> anyhow::Result<()> {
    let p = DispatchPayload::load(&payload)?;
    match p.manifest_type_or_default() {
        ManifestType::PackageXml => recipe(
            repo_root,
            p.package.clone(),
            p.version.clone(),
            p.sha.as_str().to_string(),
        ),
        ManifestType::PixiToml => pixi(
            repo_root,
            p.package.clone(),
            p.source_url(),
            p.sha.as_str().to_string(),
            p.subdir.clone(),
        ),
    }
}

fn open_pr(repo_root: Option<PathBuf>, payload: PathBuf) -> anyhow::Result<()> {
    let p = DispatchPayload::load(&payload)?;
    let repo = Repo::or_discover(repo_root)?;
    let root = repo.root();

    let branch = format!("bump/{}", p.package);
    let title = format!("chore(recipes): bump {} to {}", p.package, p.version);
    let body = format!("Automated bump from {}@{}", p.source_repo, p.sha);

    crate::process::run_in(root, "git", &["checkout", "-B", &branch])?;
    crate::process::run_in(
        root,
        "git",
        &[
            "add",
            "rosdistro_additional_recipes.yaml",
            "pixi_native_packages.yaml",
        ],
    )?;

    // `git diff --cached --quiet` exits 1 if there are staged changes; 0 if not.
    let diff = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(root)
        .status()
        .context("run git diff --cached --quiet")?;
    if diff.success() {
        // Nothing staged.
        eprintln!(
            "::notice::no changes to commit (recipes already at {})",
            p.version
        );
        return Ok(());
    }

    crate::process::run_in(root, "git", &["commit", "-m", &title])?;
    crate::process::run_in(root, "git", &["push", "--force", "origin", &branch])?;

    // Look up an existing open PR with this branch.
    let out = std::process::Command::new("gh")
        .args([
            "pr", "list", "--head", &branch, "--state", "open", "--json", "url", "-q", ".[0].url",
        ])
        .current_dir(root)
        .output()
        .context("run gh pr list")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh pr list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let pr_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let pr_url = if pr_url.is_empty() {
        let out = std::process::Command::new("gh")
            .args([
                "pr", "create", "--base", "main", "--head", &branch, "--title", &title, "--body",
                &body,
            ])
            .current_dir(root)
            .output()
            .context("run gh pr create")?;
        if !out.status.success() {
            anyhow::bail!(
                "gh pr create failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        crate::process::run_in(
            root,
            "gh",
            &["pr", "edit", &pr_url, "--title", &title, "--body", &body],
        )?;
        pr_url
    };

    crate::process::run_in(root, "gh", &["pr", "merge", &pr_url, "--auto", "--squash"])?;
    eprintln!("::notice::PR ready and auto-merge enabled: {pr_url}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
px4_msgs:
  url: https://github.com/example/px4_msgs.git
  tag: 1.3.0-amd64
  version: 1.3.0
  manifest_file: package.xml

# Comment between entries — must be preserved.
foo_pkg:
  url: https://github.com/example/foo_pkg.git
  branch: main
  version: 0.1.0
  manifest_file: package.xml

bar_pkg:
  url: https://github.com/example/bar_pkg.git
  rev: 1111111111111111111111111111111111111111
  version: 0.5.0
  manifest_file: package.xml
";

    #[test]
    fn recipe_replaces_tag_with_rev_and_updates_version() {
        let result = mutate_recipe_entry(
            FIXTURE,
            "px4_msgs",
            "1.4.0",
            "2222222222222222222222222222222222222222",
        )
        .unwrap();
        assert!(result.contains("px4_msgs:"));
        assert!(
            !result.contains("tag: 1.3.0-amd64"),
            "tag should be removed:\n{result}"
        );
        assert!(result.contains("rev: 2222222222222222222222222222222222222222"));
        assert!(result.contains("version: 1.4.0"));
        // Untouched entries.
        assert!(result.contains("foo_pkg:"));
        assert!(result.contains("# Comment between entries"));
    }

    #[test]
    fn recipe_replaces_branch_with_rev() {
        let result = mutate_recipe_entry(
            FIXTURE,
            "foo_pkg",
            "0.2.0",
            "3333333333333333333333333333333333333333",
        )
        .unwrap();
        assert!(!result.contains("branch: main"));
        assert!(result.contains("rev: 3333333333333333333333333333333333333333"));
        assert!(result.contains("version: 0.2.0"));
    }

    #[test]
    fn recipe_updates_existing_rev_in_place() {
        let result = mutate_recipe_entry(
            FIXTURE,
            "bar_pkg",
            "0.6.0",
            "4444444444444444444444444444444444444444",
        )
        .unwrap();
        assert!(!result.contains("rev: 1111111111111111111111111111111111111111"));
        assert!(result.contains("rev: 4444444444444444444444444444444444444444"));
        assert!(result.contains("version: 0.6.0"));
    }

    #[test]
    fn recipe_rejects_missing_package() {
        let err =
            mutate_recipe_entry(FIXTURE, "no_such", "0.0.1", "0".repeat(40).as_str()).unwrap_err();
        assert!(format!("{err:#}").contains("no_such"));
    }

    #[test]
    fn recipe_preserves_trailing_newline() {
        let result = mutate_recipe_entry(
            FIXTURE,
            "px4_msgs",
            "1.4.0",
            "2222222222222222222222222222222222222222",
        )
        .unwrap();
        assert!(result.ends_with('\n'));
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

    #[test]
    fn recipe_preserves_blank_line_between_entries() {
        // FIXTURE has a blank line between foo_pkg and bar_pkg.
        // After bumping foo_pkg, that blank must still exist between them.
        let out = mutate_recipe_entry(
            FIXTURE,
            "foo_pkg",
            "0.2.0",
            "8888888888888888888888888888888888888888",
        )
        .unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let foo_idx = lines.iter().position(|l| l.trim() == "foo_pkg:").unwrap();
        let bar_idx = lines.iter().position(|l| l.trim() == "bar_pkg:").unwrap();
        assert!(
            lines[foo_idx + 1..bar_idx]
                .iter()
                .any(|l| l.trim().is_empty()),
            "blank line between foo_pkg and bar_pkg lost: {out}"
        );
    }

    #[test]
    fn dispatch_payload_defaults_manifest_type_to_pixi_toml() {
        let json = r#"{
            "package": "foo",
            "version": "1.0.0",
            "source_repo": "owner/repo",
            "sha": "0000000000000000000000000000000000000000"
        }"#;
        let p: DispatchPayload = serde_json::from_str(json).unwrap();
        assert_eq!(p.manifest_type_or_default(), ManifestType::PixiToml);
    }

    #[test]
    fn dispatch_payload_parses_package_xml() {
        let json = r#"{
            "package": "foo",
            "version": "1.0.0",
            "source_repo": "owner/repo",
            "sha": "0000000000000000000000000000000000000000",
            "manifest_type": "package.xml"
        }"#;
        let p: DispatchPayload = serde_json::from_str(json).unwrap();
        assert_eq!(p.manifest_type_or_default(), ManifestType::PackageXml);
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
        let no_rev = VENDORED_FIXTURE.replace(
            "  rev: 4bcfd421c52387b3f7872b23e60059e521176f35\n",
            "",
        );
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
    fn dispatch_payload_rejects_short_sha() {
        let json = r#"{"package":"foo","version":"1.0","source_repo":"a/b","sha":"deadbeef"}"#;
        let r: Result<DispatchPayload, _> = serde_json::from_str(json);
        assert!(r.is_err());
    }

    #[test]
    fn dispatch_payload_source_url() {
        let p = DispatchPayload {
            package: "foo".into(),
            version: "1.0".into(),
            source_repo: "owner/repo".into(),
            sha: Sha40::new("0000000000000000000000000000000000000000").unwrap(),
            manifest_type: None,
            subdir: None,
        };
        assert_eq!(p.source_url(), "https://github.com/owner/repo");
    }
}
