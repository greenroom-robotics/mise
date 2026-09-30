use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::manifest::{Package, normalize};
use crate::types::PackageName;

/// Sibling dependency graph for one repo's per-package pixi workspaces.
#[derive(Debug, Default)]
pub struct SiblingGraph {
    /// package name -> package dir (parent of its pixi.toml)
    pub dirs: BTreeMap<PackageName, PathBuf>,
    /// consumer name -> sibling names referenced via `path =` deps
    pub path_deps: BTreeMap<PackageName, BTreeSet<PackageName>>,
    /// consumer name -> sibling names referenced via version pins
    pub pin_deps: BTreeMap<PackageName, BTreeSet<PackageName>>,
}

/// Build the sibling graph from packages already parsed by discovery. Every
/// dependency table in [`crate::manifest::DEP_TABLES`] is scanned.
#[must_use]
pub fn analyze(packages: &[Package]) -> SiblingGraph {
    let mut g = SiblingGraph {
        dirs: packages
            .iter()
            .map(|pkg| (pkg.manifest.name().clone(), normalize(&pkg.dir)))
            .collect(),
        ..Default::default()
    };

    let dir_to_name: BTreeMap<PathBuf, PackageName> =
        g.dirs.iter().map(|(n, d)| (d.clone(), n.clone())).collect();

    for pkg in packages {
        let name = pkg.manifest.name();
        let dir = &normalize(&pkg.dir);
        for dep in pkg.manifest.deps() {
            if dep.path().is_some() {
                let Some(path) = dep.sibling_path(dir) else {
                    continue;
                };
                if let Some(sib) = dir_to_name.get(&normalize(&dir.join(path))) {
                    g.path_deps
                        .entry(name.clone())
                        .or_default()
                        .insert(sib.clone());
                }
            } else if g.dirs.contains_key(&dep.name) && &dep.name != name {
                g.pin_deps
                    .entry(name.clone())
                    .or_default()
                    .insert(dep.name.clone());
            }
        }
    }
    g
}

#[cfg(test)]
#[path = "siblings_tests.rs"]
mod tests;
