use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::manifest::Package;
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
pub fn analyze(packages: &[Package]) -> SiblingGraph {
    let mut g = SiblingGraph::default();
    let mut named: Vec<(PackageName, PathBuf, &Package)> = Vec::new();

    for pkg in packages {
        let dir = normalize(&pkg.dir);
        let name = pkg.manifest.name().clone();
        g.dirs.insert(name.clone(), dir.clone());
        named.push((name, dir, pkg));
    }

    // dir -> name, for resolving path deps to sibling packages.
    let dir_to_name: BTreeMap<PathBuf, PackageName> =
        g.dirs.iter().map(|(n, d)| (d.clone(), n.clone())).collect();

    for (name, dir, pkg) in &named {
        for dep in pkg.manifest.deps() {
            if let Some(path) = dep.path() {
                let target = normalize(&dir.join(path));
                if &target == dir {
                    continue; // self-as-workspace-member idiom
                }
                if let Some(sib) = dir_to_name.get(&target) {
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

/// Lexical path normalization (no fs access): resolves `.` and `..`.
pub(crate) fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
#[path = "siblings_tests.rs"]
mod tests;
