use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use crate::manifest::Package;

/// Sibling dependency graph for one repo's per-package pixi workspaces.
#[derive(Debug, Default)]
pub struct SiblingGraph {
    /// package name -> package dir (parent of its pixi.toml)
    pub dirs: BTreeMap<String, PathBuf>,
    /// consumer name -> sibling names referenced via `path =` deps
    pub path_deps: BTreeMap<String, BTreeSet<String>>,
    /// consumer name -> sibling names referenced via version pins
    pub pin_deps: BTreeMap<String, BTreeSet<String>>,
}

/// Build the sibling graph from packages already parsed by discovery. Every
/// dependency table in [`crate::manifest::DEP_TABLES`] is scanned.
pub fn analyze(packages: &[Package]) -> Result<SiblingGraph> {
    let mut g = SiblingGraph::default();
    let mut named: Vec<(String, PathBuf, &Package)> = Vec::new();

    for pkg in packages {
        let dir = normalize(&pkg.dir);
        let name = pkg
            .manifest
            .name()
            .map(str::to_string)
            .or_else(|| package_xml_name(&dir))
            .with_context(|| {
                format!(
                    "{}: missing package.name and no <name> in {}",
                    pkg.manifest_path.display(),
                    dir.join("package.xml").display()
                )
            })?;
        g.dirs.insert(name.clone(), dir.clone());
        named.push((name, dir, pkg));
    }

    // dir -> name, for resolving path deps to sibling packages.
    let dir_to_name: BTreeMap<PathBuf, String> =
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
    Ok(g)
}

/// Package-xml mode: `pixi.toml [package]` has no `name` key, and identity
/// comes from a `package.xml` beside the manifest. Extracts the first
/// `<name>...</name>` element's text, or `None` if there's no `package.xml`
/// or no `<name>` element in it.
fn package_xml_name(dir: &Path) -> Option<String> {
    static NAME_RE: OnceLock<Regex> = OnceLock::new();
    let re = NAME_RE.get_or_init(|| Regex::new(r"(?s)<name>\s*(.*?)\s*</name>").unwrap());
    let text = std::fs::read_to_string(dir.join("package.xml")).ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
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
