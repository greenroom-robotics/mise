//! `mise build-recipes pixi` — the pixi-native pipeline.
//!
//! Two phases. The check phase fans out over the manifest's entries, fetches
//! each one's `pixi.toml` at its pinned rev and decides whether the artifact it
//! would produce is already published; the build phase then checks out and
//! publishes what is left, in the order [`super::plan`] settled on.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::consts::PIXI_TOML;
use crate::gh;
use crate::git;
use crate::manifest::{PackageManifest, prepend_channels, resolve_path_deps, set_build_number};
use crate::process;
use crate::repo::Repo;
use crate::types::{
    Arch, ChannelUrl, GithubRepoUrl, LocalChannel, PackageName, PixiNativeEntry, RemoteChannel,
    RunnerSize, Sha40, Version,
};

use super::channel::{BuildSubdir, ChannelIndexCache, publish_argv, version_published};
use super::local_deps::{
    LocalBuildCtx, build_local_dep, push_unique, resolve_sibling_pins, sibling_subdirs,
};
use super::plan::{BuildItem, BuildPlan};

/// The manifest path of an entry relative to its repo root: the entry's subdir
/// (when it has a meaningful one) plus `pixi.toml`.
fn entry_manifest_rel_path(entry: &PixiNativeEntry) -> String {
    entry
        .subdir
        .as_deref()
        .map(|p| p.to_string_lossy().trim_matches('/').to_string())
        .filter(|s| !s.is_empty() && s != ".")
        .map(|s| format!("{s}/{PIXI_TOML}"))
        .unwrap_or_else(|| PIXI_TOML.to_string())
}

/// Fetch an entry's `pixi.toml` at its pinned rev without cloning.
fn fetch_pixi_toml(entry: &PixiNativeEntry) -> anyhow::Result<String> {
    gh::fetch_raw_file(
        entry.url.owner(),
        entry.url.repo(),
        entry.rev.as_str(),
        &entry_manifest_rel_path(entry),
    )
    .with_context(|| format!("entry {}", entry.name))
}

/// Materialize one commit of an entry's repo in `dest`.
fn fetch_at_rev(url: &GithubRepoUrl, rev: &Sha40, dest: &Path) -> anyhow::Result<()> {
    git::fetch_rev(dest, &url.https_url(), rev)
}

enum CheckOutcome {
    Build {
        name: PackageName,
        version: Version,
        upstream_build: u64,
        effective_build: u64,
        upstream: Box<PackageManifest>,
    },
    SkipPlatformUnsupported {
        name: PackageName,
        version: Version,
    },
    SkipNoarchNonCanonical {
        name: PackageName,
        version: Version,
    },
    SkipAlreadyPublished {
        name: PackageName,
        version: Version,
        channels: Vec<RemoteChannel>,
    },
}

fn check_entry(
    entry: &PixiNativeEntry,
    channels: &ChannelIndexCache,
    channel: &RemoteChannel,
    routing_rules: &[crate::routing::RoutingRule],
    target_platform: Arch,
    rebuild_epoch: u64,
) -> anyhow::Result<CheckOutcome> {
    let pixi_toml_text = fetch_pixi_toml(entry)?;
    let upstream = PackageManifest::parse(&pixi_toml_text)
        .with_context(|| format!("entry {}: parse upstream pixi.toml", entry.name))?;
    let id = upstream
        .identity()
        .with_context(|| format!("entry {}: upstream pixi.toml", entry.name))?;

    if !upstream.supports_platform(target_platform) {
        return Ok(CheckOutcome::SkipPlatformUnsupported {
            name: id.name,
            version: id.version,
        });
    }

    // noarch artifacts are arch-independent; build them once on linux-64 and
    // skip every other platform rather than repeating the (identical) build.
    if upstream.is_noarch() && target_platform != Arch::Linux64 {
        return Ok(CheckOutcome::SkipNoarchNonCanonical {
            name: id.name,
            version: id.version,
        });
    }

    let upstream_build = upstream.build_number();
    let effective_build = upstream_build + rebuild_epoch;

    // Routed packages (see routing.yaml) publish to product channels, never
    // to the default channel — search where they actually land. Skip only
    // when every routed channel has the build (a partially-drained
    // multi-channel publish, e.g. dual-publish rules, should re-run).
    let published =
        crate::routing::published_channels(routing_rules, channel, &id.name, &id.version);
    let subdir = BuildSubdir::of(&upstream, target_platform);
    let mut published_everywhere = true;
    for c in &published {
        published_everywhere &=
            channels
                .get(c)?
                .has_build(&id.name, &id.version, effective_build, subdir);
    }
    if published_everywhere {
        return Ok(CheckOutcome::SkipAlreadyPublished {
            name: id.name,
            version: id.version,
            channels: published,
        });
    }

    Ok(CheckOutcome::Build {
        name: id.name,
        version: id.version,
        upstream_build,
        effective_build,
        upstream: Box::new(upstream),
    })
}

/// Select entries to build: keep those matching `runner_size` (when set) and,
/// when `only` is non-empty, only those whose name is listed.
fn select_entries<'a>(
    packages: &'a [PixiNativeEntry],
    runner_size: Option<RunnerSize>,
    only: &[PackageName],
) -> Vec<&'a PixiNativeEntry> {
    packages
        .iter()
        .filter(|e| runner_size.is_none_or(|s| e.runner_size == s))
        .filter(|e| only.is_empty() || only.iter().any(|n| n == &e.name))
        .collect()
}

pub(super) fn pixi(
    repo_root: Option<PathBuf>,
    channel: RemoteChannel,
    output_dir: PathBuf,
    target_platform: Arch,
    runner_size: Option<RunnerSize>,
    only: &[PackageName],
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let manifest = repo.pixi_native_manifest()?;

    // Entry checkouts (`fetch_at_rev`) and their sibling path deps come from
    // private repos over HTTPS; mise owns that auth setup.
    gh::ensure_git_auth()?;

    let abs_output = if output_dir.is_absolute() {
        output_dir
    } else {
        repo.root().join(&output_dir)
    };
    fs::create_dir_all(&abs_output).with_context(|| format!("mkdir {}", abs_output.display()))?;

    if manifest.packages.is_empty() {
        tracing::info!(
            "{} has no entries; nothing to build",
            crate::consts::PIXI_NATIVE_PACKAGES_YAML
        );
        return Ok(());
    }

    let filtered = select_entries(&manifest.packages, runner_size, only);

    if filtered.is_empty() {
        return Ok(());
    }

    let channel_ref = &channel;
    let routing_rules = crate::routing::load_rules(repo.root())?;
    let routing_rules_ref: &[crate::routing::RoutingRule] = &routing_rules;
    // Shared across the fan-out so each channel is swept once for the whole
    // job instead of once per entry.
    let channels = ChannelIndexCache::new(target_platform);
    let channels_ref = &channels;
    let rebuild_epoch = manifest.rebuild_epoch;
    let outcomes: Vec<(&PixiNativeEntry, anyhow::Result<CheckOutcome>)> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = filtered
                .iter()
                .copied()
                .map(|entry| {
                    scope.spawn(move || {
                        check_entry(
                            entry,
                            channels_ref,
                            channel_ref,
                            routing_rules_ref,
                            target_platform,
                            rebuild_epoch,
                        )
                    })
                })
                .collect();
            filtered
                .iter()
                .copied()
                .zip(handles)
                .map(|(entry, h)| (entry, h.join().expect("check thread panicked")))
                .collect()
        });

    let mut to_build: Vec<BuildItem> = Vec::new();
    let mut build_labels: Vec<String> = Vec::new();
    for (entry, outcome) in outcomes {
        match outcome? {
            CheckOutcome::Build {
                name,
                version,
                upstream_build,
                effective_build,
                upstream,
            } => {
                if rebuild_epoch > 0 {
                    build_labels.push(format!(
                        "{name} {version} build={effective_build} \
                         (upstream={upstream_build}+epoch={rebuild_epoch})"
                    ));
                } else {
                    build_labels.push(format!("{name} {version}"));
                }
                to_build.push(BuildItem {
                    entry,
                    effective_build,
                    name: name.clone(),
                    rel_path_deps: upstream.path_dep_rel_paths(),
                    // Dep keys of exact `==X.Y.Z` pins. Mirrors `rel_path_deps`
                    // above for the committed-pin farm ordering rule.
                    pin_dep_names: upstream.exact_pins().into_iter().map(|(k, _)| k).collect(),
                });
            }
            CheckOutcome::SkipPlatformUnsupported { name, version } => {
                tracing::info!(
                    "skipping {name} {version}: pixi.toml does not list {target_platform}",
                );
            }
            CheckOutcome::SkipNoarchNonCanonical { name, version } => {
                tracing::info!(
                    "skipping {name} {version}: noarch, built only on linux-64 \
                     (not {target_platform})",
                );
            }
            CheckOutcome::SkipAlreadyPublished {
                name,
                version,
                channels,
            } => {
                tracing::info!(
                    "skipping {name} {version}: already in channel(s) {}",
                    channels
                        .iter()
                        .map(RemoteChannel::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
        }
    }

    // End of the check phase: everything below consumes the plan and performs
    // side effects, so the dependency order (and any cycle) is settled here.
    // A cycle is reported with the entries that were in the failing set — the
    // ordinary "building N entries" line below is never reached in that case,
    // and a cycle is undiagnosable without knowing what was being ordered.
    let queued = to_build.len();
    let plan = BuildPlan::new(to_build)
        .with_context(|| format!("ordering {queued} entries: {}", build_labels.join(", ")))?;
    if plan.is_empty() {
        tracing::info!("nothing to build");
        return Ok(());
    }

    tracing::info!(
        "building {} entries: {}",
        plan.len(),
        build_labels.join(", "),
    );

    // Dep satisfaction deliberately only consults the default channel, same as
    // before routing was introduced: routing decides where an artifact is
    // *published*, not which channels a consumer solves against.
    let default_channel = channels.get(&channel)?;

    let output_channel = LocalChannel::new(&abs_output);
    let mut built_this_job: BTreeSet<PackageName> = BTreeSet::new();
    for item in plan {
        let entry = item.entry;
        let effective_build = item.effective_build;
        tracing::debug!("building {} (build order name: {})", entry.name, item.name);
        let tmp = tempfile::Builder::new()
            .prefix(&format!("pixi-native-{}-", entry.name))
            .tempdir()
            .context("create temp workdir")?;
        let workdir = tmp.path().join("src");
        fs::create_dir(&workdir)?;
        fetch_at_rev(&entry.url, &entry.rev, &workdir)?;

        let subdir = entry.subdir_or_root();
        let manifest_dir = workdir.join(subdir);
        let manifest_path = manifest_dir.join(PIXI_TOML);
        if !manifest_path.is_file() {
            anyhow::bail!(
                "entry {}: no pixi.toml at {}/pixi.toml in checkout",
                entry.name,
                subdir.display(),
            );
        }

        if rebuild_epoch > 0 {
            set_build_number(&manifest_path, effective_build).with_context(|| {
                format!(
                    "entry {}: rewrite build-number to {effective_build}",
                    entry.name
                )
            })?;
        }

        // Resolve path deps ephemerally in the temp checkout: derived pins in
        // the published artifact, and local channels for anything the real
        // channel can't satisfy yet.
        let local_deps_dir = tmp.path().join("local-deps");
        let sib_subdirs = sibling_subdirs(entry, &manifest.packages);
        // Read committed `==` pins before resolve_path_deps rewrites path deps
        // into pins (else those freshly-written pins would be re-detected here).
        let sibling_pins = resolve_sibling_pins(&manifest_path, &workdir, &sib_subdirs)?;
        let mut resolved = resolve_path_deps(&manifest_path)?;
        resolved.extend(sibling_pins);
        let mut extra_channels: Vec<ChannelUrl> = Vec::new();
        let mut local_built: BTreeSet<PackageName> = BTreeSet::new();
        let mut visiting: Vec<PackageName> = Vec::new();
        let local_ctx = LocalBuildCtx {
            local_deps_dir: &local_deps_dir,
            channel: &default_channel,
            target_platform,
            workdir: &workdir,
            sibling_subdirs: &sib_subdirs,
        };
        for dep in &resolved {
            // The output channel gains packages as this loop publishes into it,
            // so it has to be asked live rather than swept.
            let in_output_channel = built_this_job.contains(&dep.name)
                || version_published(&dep.name, &dep.version, &output_channel, target_platform)?;
            if in_output_channel {
                push_unique(&mut extra_channels, output_channel.clone().into());
            } else if default_channel.has_version(&dep.name, &dep.version) {
                // Satisfied by the real channel; nothing to do.
            } else {
                // Fallback: build the sibling from this same checkout (correct
                // rev by construction) into a local-only channel. Not drained;
                // the sibling's own entry / linux-64 stays the canonical publisher.
                tracing::info!(
                    "entry {}: sibling {} floor {} not in channel and not built this job; fallback local build",
                    entry.name,
                    dep.name,
                    dep.version,
                );
                build_local_dep(dep, &local_ctx, &mut local_built, &mut visiting)?;
                push_unique(
                    &mut extra_channels,
                    LocalChannel::new(&local_deps_dir).into(),
                );
            }
        }
        if !extra_channels.is_empty() {
            prepend_channels(&manifest_path, &extra_channels)?;
        }

        // No lockfile gate before publish: like conda-forge, a source build
        // re-resolves build/host/run from the manifest + current channels.
        // `pixi publish` re-resolves regardless, and the backend re-derives
        // package metadata at build time (e.g. ament_python noarch run-deps),
        // so a committed pixi.lock written by an older backend would spuriously
        // fail `--locked` here even when the build is fine. The manifest, not
        // pixi.lock, is the source of truth for the published artifact.

        // --target-channel (not --to): pixi v0.68's `--to` flat-copies and breaks
        // the upload-artifact glob.
        let target_channel = output_channel.to_string();
        let arch = target_platform.to_string();
        process::run(
            "pixi",
            &publish_argv(&manifest_path, &target_channel, &arch),
        )?;
        built_this_job.insert(item.name.clone());
    }

    Ok(())
}

#[cfg(test)]
#[path = "pixi_tests.rs"]
mod tests;
