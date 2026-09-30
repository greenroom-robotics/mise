//! Pin a package's `path =` siblings and build them into a local file channel,
//! so the package can be published against the pinned versions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

use crate::manifest::{Package, ResolvedDep, lists_channel, prepend_channels, resolve_path_deps};
use crate::types::{LocalChannel, PackageName, SiblingPinStyle};

const PIN_STYLE: SiblingPinStyle = SiblingPinStyle::Range;

/// A manifest path with symlinks and `..` resolved, so two spellings of one
/// manifest compare equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CanonicalManifest(PathBuf);

impl CanonicalManifest {
    fn new(manifest: &Path) -> Result<Self> {
        std::fs::canonicalize(manifest)
            .map(Self)
            .with_context(|| format!("resolving {}", manifest.display()))
    }
}

struct Visit {
    name: PackageName,
    manifest: CanonicalManifest,
}

/// Pins and builds path-dep siblings into `channel`, each at most once for
/// the lifetime of the value.
pub struct LocalDeps<P> {
    channel: LocalChannel,
    pinned: BTreeSet<CanonicalManifest>,
    built: BTreeSet<CanonicalManifest>,
    publish: P,
}

impl<P: FnMut(&Path) -> Result<()>> LocalDeps<P> {
    pub const fn new(channel: LocalChannel, publish: P) -> Self {
        Self {
            channel,
            pinned: BTreeSet::new(),
            built: BTreeSet::new(),
            publish,
        }
    }

    /// Rewrites the package's `path =` deps to pins, builds each sibling
    /// (recursively) into the channel, and prepends the channel to every
    /// manifest that had path deps.
    pub fn prepare(&mut self, pkg: &Package) -> Result<()> {
        let manifest = CanonicalManifest::new(&pkg.manifest_path)?;
        let mut visiting = vec![Visit {
            name: pkg.manifest.name().clone(),
            manifest: manifest.clone(),
        }];
        self.pin(&pkg.manifest_path, manifest, &mut visiting)
    }

    fn pin(
        &mut self,
        path: &Path,
        manifest: CanonicalManifest,
        visiting: &mut Vec<Visit>,
    ) -> Result<()> {
        if self.pinned.contains(&manifest) {
            return Ok(());
        }
        if lists_channel(path, &self.channel)? {
            color_eyre::eyre::bail!(
                "{} already lists {}: it was rewritten by a previous `mise ci build`; \
                 reset the checkout",
                path.display(),
                self.channel
            );
        }
        let siblings = resolve_path_deps(path, PIN_STYLE)?;
        for dep in &siblings {
            tracing::info!(
                "{}: pinned path dep {} to {}",
                path.display(),
                dep.name,
                PIN_STYLE.pin(&dep.version)
            );
            self.build(dep, visiting)?;
        }
        if !siblings.is_empty() {
            prepend_channels(path, std::slice::from_ref(self.channel.url()))?;
            tracing::info!("{}: prepended channel {}", path.display(), self.channel);
        }
        self.pinned.insert(manifest);
        Ok(())
    }

    fn build(&mut self, dep: &ResolvedDep, visiting: &mut Vec<Visit>) -> Result<()> {
        let manifest = CanonicalManifest::new(&dep.manifest)?;
        if let Some(start) = visiting.iter().position(|v| v.manifest == manifest) {
            let cycle: Vec<&str> = visiting
                .iter()
                .skip(start)
                .map(|v| v.name.as_str())
                .chain([dep.name.as_str()])
                .collect();
            color_eyre::eyre::bail!("path dep cycle: {}", cycle.join(" -> "));
        }
        if self.built.contains(&manifest) {
            tracing::info!("sibling {} already built into {}", dep.name, self.channel);
            return Ok(());
        }
        visiting.push(Visit {
            name: dep.name.clone(),
            manifest: manifest.clone(),
        });
        self.pin(&dep.manifest, manifest.clone(), visiting)?;
        visiting.pop();
        tracing::info!(
            "building sibling {} {} into {}",
            dep.name,
            dep.version,
            self.channel
        );
        (self.publish)(&dep.manifest).with_context(|| format!("building sibling {}", dep.name))?;
        self.built.insert(manifest);
        Ok(())
    }
}

#[cfg(test)]
#[path = "local_deps_tests.rs"]
mod tests;
