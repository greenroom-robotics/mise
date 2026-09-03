//! Build order for the pixi-native pipeline.
//!
//! The check phase in [`super::pixi`] produces a [`BuildItem`] per entry that
//! needs building; [`BuildPlan`] is the topological sort of those items, so the
//! build loop there can iterate without re-deciding anything.

use std::collections::{BTreeMap, BTreeSet};

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
    pub(super) fn new(items: Vec<BuildItem<'a>>) -> color_eyre::eyre::Result<Self> {
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

fn topo_sort_builds(items: Vec<BuildItem<'_>>) -> color_eyre::eyre::Result<Vec<BuildItem<'_>>> {
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

    let mut waits_on: Vec<BTreeSet<usize>> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let (repo, subdir) = key(it.entry);
            let path_targets = it.rel_path_deps.iter().filter_map(|rel| {
                let target = normalize(&subdir.join(rel));
                index.get(&(repo.clone(), target)).copied()
            });
            let pin_targets = it.pin_dep_names.iter().filter_map(|pin| {
                name_index
                    .get(&(repo.clone(), pin.clone()))
                    .copied()
                    .filter(|&j| j != i)
            });
            path_targets.chain(pin_targets).collect()
        })
        .collect();
    let mut ready: BTreeSet<usize> = waits_on
        .iter()
        .enumerate()
        .filter(|(_, deps)| deps.is_empty())
        .map(|(i, _)| i)
        .collect();
    let mut order = Vec::new();
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for (d, deps) in waits_on.iter_mut().enumerate() {
            if deps.remove(&i) && deps.is_empty() {
                ready.insert(d);
            }
        }
    }
    if order.len() != items.len() {
        color_eyre::eyre::bail!("path-dep cycle among pixi-native entries");
    }
    let mut slots: BTreeMap<usize, BuildItem> = items.into_iter().enumerate().collect();
    Ok(order.into_iter().filter_map(|i| slots.remove(&i)).collect())
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
