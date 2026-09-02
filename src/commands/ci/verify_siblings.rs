use clap::Args;
use color_eyre::eyre::{ContextCompat, Result};
use std::path::PathBuf;

use crate::types::{PackageName, Version};

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
        use crate::commands::ci::siblings;
        use crate::manifest::{self, Package};

        let pkgs = manifest::discover(&self.package_dir, None)?;
        let graph = siblings::analyze(&pkgs);
        let consumer = Package::read(&self.pixi_toml)?.identity().name;

        let Some(targets) = graph.path_deps.get(&consumer) else {
            return Ok(());
        };

        let tags = crate::git::tags(&self.package_dir)?;
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
            if !crate::git::is_clean(&self.package_dir, &tag, "HEAD", dir)? {
                color_eyre::eyre::bail!(
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

/// Highest version among tags shaped `<pkg>@<version>`, as written in the tag.
///
/// Ordering is [`Version`]'s, i.e. semver's: release versions beat prereleases
/// at the same triple, and numeric prerelease identifiers compare numerically,
/// so `alpha.10` beats `alpha.2`.
///
/// A tag whose suffix will not parse still counts as a release — it just
/// cannot be ordered, so it ranks below every tag that will, and only wins if
/// nothing else is there. The caller reads `None` as "never released" and
/// refuses to proceed, so dropping such tags outright would turn an
/// unrecognised tag into a blocked release rather than a mis-ordered one.
pub fn latest_tagged_version<'a>(tags: &'a [String], pkg: &PackageName) -> Option<&'a str> {
    let prefix = format!("{pkg}@");
    let mut best: Option<(Version, &str)> = None;
    let mut unordered: Option<&str> = None;
    for text in tags.iter().filter_map(|t| t.strip_prefix(&prefix)) {
        match Version::parse(text) {
            Ok(v) => {
                if best.as_ref().is_none_or(|(top, _)| v >= *top) {
                    best = Some((v, text));
                }
            }
            Err(e) => {
                tracing::warn!("tag {pkg}@{text} is not orderable, ranking it last: {e:#}");
                unordered.get_or_insert(text);
            }
        }
    }
    best.map(|(_, text)| text).or(unordered)
}

#[cfg(test)]
#[path = "verify_siblings_tests.rs"]
mod tests;
