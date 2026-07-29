use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

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

/// Dependency tables scanned for sibling references. `[dependencies]` holds
/// only the self-as-workspace-member idiom today but is scanned for safety.
const DEP_TABLE_PATHS: &[&[&str]] = &[
    &["dependencies"],
    &["package", "run-dependencies"],
    &["package", "host-dependencies"],
    &["package", "build-dependencies"],
];

pub fn analyze(pixis: &[PathBuf]) -> Result<SiblingGraph> {
    let mut g = SiblingGraph::default();
    let mut docs: Vec<(String, PathBuf, toml::Value)> = Vec::new();

    for pixi in pixis {
        let text =
            std::fs::read_to_string(pixi).with_context(|| format!("reading {}", pixi.display()))?;
        let doc: toml::Value =
            toml::from_str(&text).with_context(|| format!("parsing {}", pixi.display()))?;
        let dir = normalize(pixi.parent().unwrap());
        let name = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_string)
            .or_else(|| package_xml_name(&dir))
            .with_context(|| {
                format!(
                    "{}: missing package.name and no <name> in {}",
                    pixi.display(),
                    dir.join("package.xml").display()
                )
            })?;
        g.dirs.insert(name.clone(), dir.clone());
        docs.push((name, dir, doc));
    }

    // dir -> name, for resolving path deps to sibling packages.
    let dir_to_name: BTreeMap<PathBuf, String> =
        g.dirs.iter().map(|(n, d)| (d.clone(), n.clone())).collect();

    for (name, dir, doc) in &docs {
        for table_path in DEP_TABLE_PATHS {
            let mut node = doc;
            let mut found = true;
            for seg in *table_path {
                match node.get(seg) {
                    Some(n) => node = n,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            if !found {
                continue;
            }
            let Some(table) = node.as_table() else {
                continue;
            };
            for (dep_name, value) in table {
                if let Some(path) = value.get("path").and_then(|p| p.as_str()) {
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
                } else if g.dirs.contains_key(dep_name) && dep_name != name {
                    g.pin_deps
                        .entry(name.clone())
                        .or_default()
                        .insert(dep_name.clone());
                }
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
