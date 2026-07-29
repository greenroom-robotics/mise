use anyhow::{Context, Result};
use clap::Args;
use std::path::PathBuf;

/// semantic-release prepare-step guard: every sibling referenced via a
/// `path =` dep must be byte-identical to its latest `<pkg>@<ver>` tag.
/// Runs before bump-pixi; releases in the same run are already tagged by
/// the time a dependent's prepare fires (topo order), so this single check
/// covers all safe cases. Not for direct use.
#[derive(Args, Debug)]
pub struct VerifySiblings {
    /// pixi.toml of the package about to be released.
    #[arg(long)]
    pub pixi_toml: PathBuf,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
}

impl VerifySiblings {
    pub fn run(&self) -> Result<()> {
        use crate::commands::ci::{packages, pixi_meta, siblings};

        let pixis = packages::discover(&self.package_dir, None)?;
        let graph = siblings::analyze(&pixis)?;
        let consumer = pixi_meta::read(&self.pixi_toml)?.name;

        let Some(targets) = graph.path_deps.get(&consumer) else {
            return Ok(()); // no path deps, nothing to verify
        };

        let tags = git_tags(&self.package_dir)?;
        for target in targets {
            let dir = graph
                .dirs
                .get(target)
                .with_context(|| format!("sibling {target} not found in graph"))?;
            let version = latest_tagged_version(&tags, target).with_context(|| {
                format!(
                    "sibling {target} (path dep of {consumer}) has never been released \
                     (no release tag {target}@*). Release it first."
                )
            })?;
            let tag = format!("{target}@{version}");
            if !dir_clean_at(&self.package_dir, &tag, dir)? {
                anyhow::bail!(
                    "sibling {target} has changed since {tag} and is not releasing in \
                     this run — {consumer}'s derived pin would not match published \
                     content. Remedy: give {target} a releasable commit (fix:/feat:) \
                     so it releases in the same run, or release it manually first."
                );
            }
        }
        Ok(())
    }
}

/// All git tags of the repo containing `cwd`.
fn git_tags(cwd: &std::path::Path) -> Result<Vec<String>> {
    let out = std::process::Command::new("git")
        .args(["tag", "--list"])
        .current_dir(cwd)
        .output()
        .context("running git tag --list")?;
    if !out.status.success() {
        anyhow::bail!("git tag --list failed");
    }
    Ok(String::from_utf8(out.stdout)?
        .lines()
        .map(str::to_string)
        .collect())
}

/// `git diff --quiet <tag> HEAD -- <dir>`: true when the dir is byte-identical.
fn dir_clean_at(cwd: &std::path::Path, tag: &str, dir: &std::path::Path) -> Result<bool> {
    let st = std::process::Command::new("git")
        .args(["diff", "--quiet", tag, "HEAD", "--"])
        .arg(dir)
        .current_dir(cwd)
        .status()
        .context("running git diff")?;
    match st.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => anyhow::bail!("git diff {tag} HEAD -- {} failed", dir.display()),
    }
}

/// Highest version among tags shaped `<pkg>@<version>`. Release versions beat
/// prereleases at the same numeric triple (SemVer §11); prerelease identifiers
/// compare lexically — adequate for our `alpha.N` scheme.
pub fn latest_tagged_version(tags: &[String], pkg: &str) -> Option<String> {
    let prefix = format!("{pkg}@");
    tags.iter()
        .filter_map(|t| t.strip_prefix(&prefix))
        .max_by(|a, b| version_key(a).cmp(&version_key(b)))
        .map(str::to_string)
}

/// Sort key: (numeric triple, is-release, prerelease-suffix).
fn version_key(v: &str) -> (Vec<u64>, bool, String) {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let nums = core
        .split('.')
        .map(|s| s.parse::<u64>().unwrap_or(0))
        .collect();
    (nums, pre.is_none(), pre.unwrap_or("").to_string())
}

#[cfg(test)]
#[path = "verify_siblings_tests.rs"]
mod tests;
