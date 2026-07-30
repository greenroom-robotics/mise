//! Same-repo dependencies of a pixi-native entry.
//!
//! Two jobs, both in service of the build loop in [`super::pixi`]: work out
//! which of an entry's committed `==` pins point at a sibling in the same
//! checkout, and — when neither the real channel nor this job has that
//! sibling's version — build it from that checkout into a local-only file
//! channel so the entry can solve against it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::consts::PIXI_TOML;
use crate::manifest::{PackageManifest, ResolvedDep, prepend_channels, resolve_path_deps};
use crate::process;
use crate::types::{Arch, ChannelUrl, LocalChannel, PackageName, PixiNativeEntry};

use super::channel::ChannelIndex;
use super::channel::publish_argv;

/// Resolve committed `==X.Y.Z` pins that reference a same-repo sibling, so the
/// fallback local build can satisfy a coupled cross-bucket dependency the real
/// channel hasn't drained yet. Unlike `resolve_path_deps` this rewrites
/// nothing — pins are already pins.
///
/// `sibling_subdirs` maps dep-name -> repo-relative subdir for the current
/// entry's same-repo siblings. Only pins whose key is in that map and whose
/// sibling checkout version equals the pin are returned (fallback-capable). A
/// pin to an older release (checkout version differs — normal under opt-in) is
/// skipped: the real channel must satisfy it.
pub(super) fn resolve_sibling_pins(
    manifest_path: &Path,
    workdir: &Path,
    sibling_subdirs: &BTreeMap<PackageName, PathBuf>,
) -> anyhow::Result<Vec<ResolvedDep>> {
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let upstream = PackageManifest::parse(&text)
        .with_context(|| format!("parse {}", manifest_path.display()))?;

    let mut out = Vec::new();
    for (name, pin) in upstream.exact_pins() {
        let Some(subdir) = sibling_subdirs.get(&name) else {
            continue; // not a same-repo sibling; real channel owns it
        };
        let sib_manifest = workdir.join(subdir).join(PIXI_TOML);
        let sib_text = fs::read_to_string(&sib_manifest).with_context(|| {
            format!(
                "pin dep {name}: no pixi.toml at {} in checkout",
                sib_manifest.display()
            )
        })?;
        let sib_version = PackageManifest::parse(&sib_text)
            .with_context(|| {
                format!(
                    "pin dep {name}: parsing sibling manifest {}",
                    sib_manifest.display()
                )
            })?
            .version()
            .clone();
        if sib_version == pin {
            out.push(ResolvedDep {
                name,
                version: pin,
                manifest: sib_manifest,
            });
        } else {
            tracing::info!(
                "pin dep {name} =={pin} differs from sibling checkout version \
                 {sib_version}; leaving it for the real channel",
            );
        }
    }
    Ok(out)
}

/// dep-name -> repo-relative subdir for `entry`'s same-repo siblings (excluding
/// `entry` itself). The entry `name` is the channel artifact name, i.e. the dep
/// key a consumer's pin uses.
pub(super) fn sibling_subdirs(
    entry: &PixiNativeEntry,
    all: &[PixiNativeEntry],
) -> BTreeMap<PackageName, PathBuf> {
    all.iter()
        .filter(|e| e.url == entry.url && e.name != entry.name)
        .map(|e| {
            (
                e.name.clone(),
                e.subdir.clone().unwrap_or_else(|| PathBuf::from(".")),
            )
        })
        .collect()
}

/// Push `c` onto `v` unless it's already present.
pub(super) fn push_unique(v: &mut Vec<ChannelUrl>, c: ChannelUrl) {
    if !v.contains(&c) {
        v.push(c);
    }
}

/// Guard for `build_local_dep`'s recursion, factored out so it's testable
/// without invoking pixi. `Ok(false)` means skip (already built this entry),
/// `Ok(true)` means proceed, `Err` means a cycle was detected among the
/// sibling path deps being built as local fallbacks.
fn check_local_build_guard(
    name: &PackageName,
    visiting: &[PackageName],
    local_built: &BTreeSet<PackageName>,
) -> anyhow::Result<bool> {
    if visiting.contains(name) {
        anyhow::bail!(
            "path-dep cycle among local fallback builds: {} -> {}",
            visiting
                .iter()
                .map(PackageName::as_str)
                .collect::<Vec<_>>()
                .join(" -> "),
            name
        );
    }
    if local_built.contains(name) {
        return Ok(false);
    }
    Ok(true)
}

/// Immutable context shared across a `build_local_dep` recursion (fixed for
/// one top-level entry). Split out to keep the recursive fn's arg count sane.
pub(super) struct LocalBuildCtx<'a> {
    pub(super) local_deps_dir: &'a Path,
    /// Snapshot of the upstream channel, swept once for the whole job.
    pub(super) channel: &'a ChannelIndex,
    pub(super) target_platform: Arch,
    /// Repo checkout root, for resolving same-repo sibling pins.
    pub(super) workdir: &'a Path,
    /// dep-name -> repo-relative subdir for same-repo siblings.
    pub(super) sibling_subdirs: &'a BTreeMap<PackageName, PathBuf>,
}

/// Build a path-dep sibling from the consumer's checkout into a local-only
/// file channel. Recurses through the sibling's own path deps and same-repo
/// pins first. `local_built` and `visiting` are scoped per top-level entry
/// (fresh at each call site in the main build loop in [`super::pixi`]):
/// `local_built` dedupes a diamond dependency shared by two siblings, and
/// `visiting` catches a cycle among path deps before it reaches pixi's solver.
/// ponytail: duplicate build when the sibling lives in another matrix job;
/// layered farm stages are the upgrade path if this gets slow in practice.
pub(super) fn build_local_dep(
    dep: &ResolvedDep,
    ctx: &LocalBuildCtx<'_>,
    local_built: &mut BTreeSet<PackageName>,
    visiting: &mut Vec<PackageName>,
) -> anyhow::Result<()> {
    if !check_local_build_guard(&dep.name, visiting, local_built)? {
        return Ok(());
    }
    fs::create_dir_all(ctx.local_deps_dir)?;
    visiting.push(dep.name.clone());
    // Same read-before-rewrite ordering as the main build loop in `super::pixi`.
    let sibling_pins = resolve_sibling_pins(&dep.manifest, ctx.workdir, ctx.sibling_subdirs)?;
    let mut nested = resolve_path_deps(&dep.manifest)?;
    nested.extend(sibling_pins);
    let mut built_any_nested = false;
    for n in &nested {
        if !ctx.channel.has_version(&n.name, &n.version) {
            build_local_dep(n, ctx, local_built, visiting)?;
            built_any_nested = true;
        }
    }
    visiting.pop();
    let target_channel = LocalChannel::new(ctx.local_deps_dir);
    if built_any_nested {
        prepend_channels(&dep.manifest, &[target_channel.clone().into()])?;
    }
    let target_channel = target_channel.to_string();
    let arch = ctx.target_platform.to_string();
    process::run("pixi", &publish_argv(&dep.manifest, &target_channel, &arch))?;
    local_built.insert(dep.name.clone());
    Ok(())
}

#[cfg(test)]
#[path = "local_deps_tests.rs"]
mod tests;
