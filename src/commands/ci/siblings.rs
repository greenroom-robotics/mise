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

/// Kahn's algorithm over path_deps ∪ pin_deps. Dependencies come before
/// dependents. Deterministic (BTree iteration order). Errors on cycles.
pub fn topo_order(graph: &SiblingGraph) -> Result<Vec<String>> {
    let mut indegree: BTreeMap<&str, usize> = graph.dirs.keys().map(|n| (n.as_str(), 0)).collect();
    let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (consumer, targets) in graph.path_deps.iter().chain(graph.pin_deps.iter()) {
        for t in targets {
            if graph.dirs.contains_key(t) {
                if dependents
                    .entry(t.as_str())
                    .or_default()
                    .insert(consumer.as_str())
                {
                    *indegree.get_mut(consumer.as_str()).unwrap() += 1;
                }
            }
        }
    }
    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| *n)
        .collect();
    let mut order = Vec::new();
    while let Some(&n) = ready.iter().next() {
        ready.remove(n);
        order.push(n.to_string());
        for d in dependents.get(n).cloned().unwrap_or_default() {
            let deg = indegree.get_mut(d).unwrap();
            *deg -= 1;
            if *deg == 0 {
                ready.insert(d);
            }
        }
    }
    if order.len() != graph.dirs.len() {
        let stuck: Vec<&str> = indegree
            .iter()
            .filter(|(_, d)| **d > 0)
            .map(|(n, _)| *n)
            .collect();
        anyhow::bail!("sibling dependency cycle involving: {}", stuck.join(", "));
    }
    Ok(order)
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_pkg(root: &std::path::Path, name: &str, extra: &str) -> std::path::PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let body = format!(
            "[workspace]\nname = \"{name}\"\n\n[dependencies]\n{name} = {{ path = \".\" }}\n\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{extra}"
        );
        let p = dir.join("pixi.toml");
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn detects_path_dep_between_siblings() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(tmp.path(), "geolocation", "");
        let b = write_pkg(
            tmp.path(),
            "geolocation_node",
            "[package.run-dependencies]\ngeolocation = { path = \"../geolocation\" }\n",
        );
        let g = analyze(&[a, b]).unwrap();
        assert!(g.path_deps["geolocation_node"].contains("geolocation"));
        assert!(g.path_deps.get("geolocation").is_none_or(|s| s.is_empty()));
    }

    #[test]
    fn self_path_dep_idiom_is_ignored() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(tmp.path(), "solo", "");
        let g = analyze(&[a]).unwrap();
        assert!(g.path_deps.get("solo").is_none_or(|s| s.is_empty()));
    }

    #[test]
    fn detects_version_pin_on_sibling() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(tmp.path(), "geolocation", "");
        let b = write_pkg(
            tmp.path(),
            "geolocation_node",
            "[package.run-dependencies]\ngeolocation = \"==1.0.0\"\n",
        );
        let g = analyze(&[a, b]).unwrap();
        assert!(g.pin_deps["geolocation_node"].contains("geolocation"));
    }

    #[test]
    fn host_dependencies_also_scanned() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(tmp.path(), "geolocation_msgs", "");
        let b = write_pkg(
            tmp.path(),
            "geolocation_node",
            "[package.host-dependencies]\ngeolocation_msgs = { path = \"../geolocation_msgs\" }\n",
        );
        let g = analyze(&[a, b]).unwrap();
        assert!(g.path_deps["geolocation_node"].contains("geolocation_msgs"));
    }

    #[test]
    fn external_deps_produce_no_edges() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(
            tmp.path(),
            "geolocation",
            "[package.run-dependencies]\nros-kilted-rclpy = \"*\"\npydantic = \">=2,<3\"\n",
        );
        let g = analyze(&[a]).unwrap();
        assert!(g.pin_deps.get("geolocation").is_none_or(|s| s.is_empty()));
    }

    #[test]
    fn topo_puts_dependencies_first() {
        let tmp = TempDir::new().unwrap();
        let msgs = write_pkg(tmp.path(), "geolocation_msgs", "");
        let lib = write_pkg(
            tmp.path(),
            "geolocation",
            "[package.run-dependencies]\ngeolocation_msgs = { path = \"../geolocation_msgs\" }\n",
        );
        let node = write_pkg(
            tmp.path(),
            "geolocation_node",
            "[package.run-dependencies]\ngeolocation = { path = \"../geolocation\" }\ngeolocation_msgs = { path = \"../geolocation_msgs\" }\n",
        );
        let g = analyze(&[node, lib, msgs]).unwrap();
        let order = topo_order(&g).unwrap();
        let pos = |n: &str| order.iter().position(|x| x == n).unwrap();
        assert!(pos("geolocation_msgs") < pos("geolocation"));
        assert!(pos("geolocation") < pos("geolocation_node"));
    }

    #[test]
    fn package_xml_mode_manifest_falls_back_to_package_xml_name() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("geolocation");
        fs::create_dir_all(&dir).unwrap();
        let pixi = dir.join("pixi.toml");
        fs::write(
            &pixi,
            "[workspace]\nname = \"geolocation\"\n\n[package]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("package.xml"),
            "<?xml version=\"1.0\"?>\n<package format=\"3\">\n  <name>geolocation</name>\n  <version>1.0.0</version>\n</package>\n",
        )
        .unwrap();
        let g = analyze(&[pixi]).unwrap();
        assert!(g.dirs.contains_key("geolocation"));
    }

    #[test]
    fn missing_name_in_both_manifest_and_package_xml_errors_mentioning_both() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("geolocation");
        fs::create_dir_all(&dir).unwrap();
        let pixi = dir.join("pixi.toml");
        fs::write(
            &pixi,
            "[workspace]\nname = \"geolocation\"\n\n[package]\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let err = analyze(&[pixi]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("package.name"));
        assert!(msg.contains("package.xml"));
    }

    #[test]
    fn cycle_is_hard_error() {
        let tmp = TempDir::new().unwrap();
        let a = write_pkg(
            tmp.path(),
            "a",
            "[package.run-dependencies]\nb = { path = \"../b\" }\n",
        );
        let b = write_pkg(
            tmp.path(),
            "b",
            "[package.run-dependencies]\na = { path = \"../a\" }\n",
        );
        let g = analyze(&[a, b]).unwrap();
        let err = topo_order(&g).unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }
}
