//! Build order for the pixi-native pipeline.
//!
//! The check phase in [`super::pixi`] produces a [`BuildItem`] per entry that
//! needs building; [`BuildPlan`] is the topological sort of those items, so the
//! build loop there can iterate without re-deciding anything.

use std::collections::BTreeMap;

use crate::types::{PackageName, PixiNativeEntry};

/// A pixi-native entry selected for building, along with the info needed to
/// order it relative to other builds (see [`BuildPlan::new`]).
#[derive(Debug)]
pub(super) struct BuildItem<'a> {
    pub(super) entry: &'a PixiNativeEntry,
    pub(super) effective_build: u64,
    pub(super) name: PackageName,
    pub(super) rel_path_deps: Vec<String>,
    /// Dep keys of committed `==` pins (channel artifact names). A same-repo
    /// sibling matching one must build first (opt-out coupling still needs
    /// same-bucket ordering).
    pub(super) pin_dep_names: Vec<PackageName>,
}

/// The build order, as a value.
///
/// The only constructor is [`BuildPlan::new`], which *is* the topological sort,
/// so holding one is proof that every same-repo dependency target precedes its
/// consumers. A dependency cycle is rejected here, before any build has run.
#[derive(Debug)]
pub(super) struct BuildPlan<'a>(Vec<BuildItem<'a>>);

impl<'a> BuildPlan<'a> {
    /// Order build items so same-repo dependency targets build before
    /// consumers. Path-dep edge: `consumer.subdir/rel_path` (normalized) ==
    /// target.subdir, same url. Pin edge: consumer's `==` pin key == target
    /// `entry.name`, same url. Errors if those edges form a cycle.
    pub(super) fn new(items: Vec<BuildItem<'a>>) -> anyhow::Result<Self> {
        topo_sort_builds(items).map(Self)
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) const fn len(&self) -> usize {
        self.0.len()
    }
}

impl<'a> IntoIterator for BuildPlan<'a> {
    type Item = BuildItem<'a>;
    type IntoIter = std::vec::IntoIter<BuildItem<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

fn topo_sort_builds(items: Vec<BuildItem<'_>>) -> anyhow::Result<Vec<BuildItem<'_>>> {
    use crate::commands::ci::siblings::normalize;

    let key = |e: &PixiNativeEntry| (e.url.slug(), normalize(e.subdir_or_root()));
    let repo_of = |e: &PixiNativeEntry| e.url.slug();
    let index: BTreeMap<_, usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (key(it.entry), i))
        .collect();
    // (repo, entry.name) -> index, for pin edges keyed on the artifact name.
    let name_index: BTreeMap<(String, PackageName), usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| ((repo_of(it.entry), it.entry.name.clone()), i))
        .collect();

    let mut indegree = vec![0usize; items.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    for (i, it) in items.iter().enumerate() {
        let (repo, subdir) = key(it.entry);
        for rel in &it.rel_path_deps {
            let target = normalize(&subdir.join(rel));
            if let Some(&j) = index.get(&(repo.clone(), target)) {
                dependents[j].push(i);
                indegree[i] += 1;
            }
        }
        for pin in &it.pin_dep_names {
            if let Some(&j) = name_index.get(&(repo.clone(), pin.clone()))
                && j != i
            {
                dependents[j].push(i);
                indegree[i] += 1;
            }
        }
    }
    let mut ready: std::collections::BTreeSet<usize> = indegree
        .iter()
        .enumerate()
        .filter(|(_, d)| **d == 0)
        .map(|(i, _)| i)
        .collect();
    let mut order = Vec::new();
    while let Some(&i) = ready.iter().next() {
        ready.remove(&i);
        order.push(i);
        for &d in &dependents[i] {
            indegree[d] -= 1;
            if indegree[d] == 0 {
                ready.insert(d);
            }
        }
    }
    if order.len() != items.len() {
        anyhow::bail!("path-dep cycle among pixi-native entries");
    }
    let mut slots: Vec<Option<BuildItem>> = items.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| slots[i].take().unwrap())
        .collect())
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
