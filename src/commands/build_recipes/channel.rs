//! Talking to a conda channel: asking what it already holds, and naming one
//! as a publish target.
//!
//! The publish check and dependency satisfaction both reduce to "does this
//! channel have `name == version` (at this build number, in this subdir)". How
//! that question is answered depends on the channel: a remote one cannot
//! change while the job runs, so it is swept once into a [`ChannelIndex`] and
//! matched in memory; a local output channel gains packages as the job
//! publishes into it, so it goes through [`version_published`] live.
//!
//! [`publish_argv`] lives here rather than beside either of its callers: both
//! the main build loop and the local-dependency fallback publish, and putting
//! it in either one would make the two modules mutually dependent.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;

use crate::manifest::PackageManifest;
use crate::process;
use crate::types::{Arch, LocalChannel, PackageName, RemoteChannel, Version};

/// `pixi publish` argv. Built from `OsStr` so a non-UTF-8 manifest path is
/// passed through rather than panicking a `to_str().unwrap()`.
pub(super) fn publish_argv<'a>(
    manifest: &'a Path,
    target_channel: &'a str,
    arch: &'a str,
) -> [&'a std::ffi::OsStr; 7] {
    use std::ffi::OsStr;
    [
        OsStr::new("publish"),
        OsStr::new("--path"),
        manifest.as_os_str(),
        OsStr::new("--target-channel"),
        OsStr::new(target_channel),
        OsStr::new("--target-platform"),
        OsStr::new(arch),
    ]
}

/// Substrings in `pixi search` stderr that mean the channel could not be
/// consulted at all, as opposed to being consulted and having nothing to say.
///
/// The distinction matters because the two outcomes lead opposite ways: a
/// genuinely empty channel means "not published, go build it", while an
/// unreachable one means the publish check has no idea and building would
/// risk republishing an artifact that already exists. Matching on message text
/// is unpleasant, but `pixi search` exits non-zero for both cases and offers
/// no other signal.
const UNREACHABLE_MARKERS: &[&str] = &[
    "error sending request",
    "failed to fetch",
    "failed to download",
    "dns error",
    "connection refused",
    "connection reset",
    "operation timed out",
    "timed out",
    "certificate",
    "unauthorized",
    "403 forbidden",
    "500 internal server error",
    "502 bad gateway",
    "503 service unavailable",
];

/// Whether a failed `pixi search` means "could not reach the channel" rather
/// than "the channel has no such package".
fn channel_unreachable(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    UNREACHABLE_MARKERS.iter().any(|m| lower.contains(m))
}

/// Run `pixi search --json <spec> -c <channel> -p <arch>` and return the parsed
/// records grouped by subdir.
///
/// Three outcomes, kept distinct on purpose:
///
/// - `Ok(Some(records))` — the channel answered.
/// - `Ok(None)` — the channel answered that it has nothing matching `spec`.
///   `pixi search` reports no match with a non-zero exit, so this is a normal
///   result and callers proceed as if the package needs building.
/// - `Err(_)` — the channel could not be consulted (see
///   [`channel_unreachable`]) or produced output that could not be parsed.
///   Treating this as "nothing published" would silently rebuild and
///   republish the whole channel, so it is a hard error.
fn search_channel(
    spec: &str,
    channel_url: &str,
    target_platform: Arch,
) -> anyhow::Result<Option<BTreeMap<String, Vec<SearchRecord>>>> {
    let arch = target_platform.to_string();

    let stdout = match process::capture_probe(
        "pixi",
        &["search", "--json", spec, "-c", channel_url, "-p", &arch],
    )
    .with_context(|| format!("pixi search {spec} in {channel_url}"))?
    {
        process::Captured::Output(out) => out,
        process::Captured::Failed { code, stderr } => {
            anyhow::ensure!(
                !channel_unreachable(&stderr),
                "pixi search {spec} could not reach {channel_url} (exit {code:?}): {}",
                stderr.trim(),
            );
            tracing::info!("pixi search for {spec} in {channel_url} found nothing");
            return Ok(None);
        }
    };

    serde_json::from_str(&stdout)
        .map(Some)
        .with_context(|| format!("pixi search {spec} in {channel_url} returned non-JSON stdout"))
}

/// One record from `pixi search --json`. Only the fields the publish checks need.
#[derive(Debug, serde::Deserialize)]
struct SearchRecord {
    name: String,
    version: String,
    build_number: u64,
    subdir: String,
}

/// The channel subdir an entry's artifact lands in: `noarch` when the build is
/// arch-independent, otherwise the job's arch.
///
/// The publish check has to be told this rather than infer it. A build number
/// sitting under a *different* subdir says nothing about whether the artifact
/// we're about to produce exists — a package that moved from arch to noarch
/// mid-version has records in both, and matching the wrong one either skips a
/// build that never happened or repeats one that did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildSubdir {
    Noarch,
    Arch(Arch),
}

impl BuildSubdir {
    /// Where `upstream` publishes to when built for `target_platform`.
    pub(super) fn of(upstream: &PackageManifest, target_platform: Arch) -> Self {
        if upstream.is_noarch() {
            Self::Noarch
        } else {
            Self::Arch(target_platform)
        }
    }
}

impl fmt::Display for BuildSubdir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Noarch => f.write_str("noarch"),
            Self::Arch(a) => write!(f, "{a}"),
        }
    }
}

/// Every record in one channel for one platform, from a single
/// `pixi search --json '*'` sweep.
///
/// `pixi search` takes an exclusive advisory lock on the repodata cache entry
/// for a channel (rattler's `utils/flock.rs`), so N searches against the *same*
/// channel cost N × one-search however many threads issue them. Sweeping once
/// and matching in memory collapses the whole check phase into a single lock
/// hold: measured against the GR channel, 40 concurrent exact searches take
/// 14.3s while one sweep of all 283 packages takes 0.45s warm / ~6s cold.
///
/// Only sound for a channel that cannot change while the snapshot is held. That
/// holds for the upstream and product channels during a build job — builds
/// publish into a local `file://` output channel and are drained to the real
/// ones later. The mutable local channels keep using [`version_published`].
///
/// One of these per channel, built on demand by [`ChannelIndexCache`], since
/// routing sends different packages to different channels.
///
/// Records are keyed on the raw strings the channel reported. Neither the
/// package name nor the version is parsed into [`PackageName`] / [`Version`]:
/// a channel holds artifacts this repo does not publish, whose conda versions
/// (epochs, `1.2`, `post`/`dev` segments) are a wider grammar than semver, and
/// one unparseable record must not fail the sweep. The *queries* are typed
/// instead — a lookup can only be made with a parsed name and version, which
/// is where the invariant is actually needed.
///
/// Do not point this at a public channel: a `'*'` glob makes the gateway pull
/// the channel's full name index and then fetch records per match, which on
/// e.g. robostack (34k names) takes minutes. The GR channels are all small —
/// `general` is 283 packages, the product channels 1-2 each.
pub(super) struct ChannelIndex {
    /// `(subdir, name, version)` -> build numbers published *in that subdir*.
    /// Kept per-subdir because the publish check must ask about the one subdir
    /// it is about to write to; see [`BuildSubdir`].
    builds: HashMap<(String, String, String), Vec<u64>>,
    /// `(name, version)` present in any subdir at all. Dep satisfaction is
    /// deliberately subdir-agnostic — a noarch dependency satisfies a consumer
    /// being built for an arch.
    versions: HashSet<(String, String)>,
}

impl ChannelIndex {
    /// Sweep `channel` for `target_platform`. An *empty* channel yields an
    /// empty index — "nothing is published yet". An *unreachable* one is an
    /// error: an index that wrongly looks empty makes every package look
    /// unpublished.
    fn sweep(channel: &RemoteChannel, target_platform: Arch) -> anyhow::Result<Self> {
        let channel_url = channel.to_string();
        let index = Self::from_records(
            &search_channel("*", &channel_url, target_platform)?.unwrap_or_default(),
        );
        tracing::info!(
            "swept {channel_url} for {target_platform}: {} name/version pairs across {} subdir slots",
            index.versions.len(),
            index.builds.len(),
        );
        Ok(index)
    }

    /// Fold `pixi search --json` output into the lookup tables. Every subdir the
    /// search returned is folded in, not just the requested arch: a `noarch`
    /// package is reported under the `noarch` key even when searching with
    /// `-p linux-64`, so ignoring that key makes noarch packages look
    /// permanently unpublished.
    fn from_records(parsed: &BTreeMap<String, Vec<SearchRecord>>) -> Self {
        let mut builds: HashMap<(String, String, String), Vec<u64>> = HashMap::new();
        let mut versions: HashSet<(String, String)> = HashSet::new();
        for r in parsed.values().flatten() {
            builds
                .entry((r.subdir.clone(), r.name.clone(), r.version.clone()))
                .or_default()
                .push(r.build_number);
            versions.insert((r.name.clone(), r.version.clone()));
        }
        Self { builds, versions }
    }

    /// Whether `name == version` is published in `subdir` with exactly
    /// `build_number` — i.e. whether the artifact this job would produce is
    /// already there. Records under any other subdir are ignored on purpose.
    pub(super) fn has_build(
        &self,
        name: &PackageName,
        version: &Version,
        build_number: u64,
        subdir: BuildSubdir,
    ) -> bool {
        self.builds
            .get(&(subdir.to_string(), name.to_string(), version.to_string()))
            .is_some_and(|b| b.contains(&build_number))
    }

    /// Whether *any* build of `name == version` is published, in any subdir.
    /// For dep satisfaction we only care that some build of the pinned version
    /// is available to solve against.
    pub(super) fn has_version(&self, name: &PackageName, version: &Version) -> bool {
        self.versions
            .contains(&(name.to_string(), version.to_string()))
    }
}

/// Sweeps each channel at most once per job and hands the snapshot to every
/// caller that asks for it.
///
/// The set of channels isn't known before the check fan-out: routing rules map
/// a package to its product channels, and the package's version only arrives
/// with the upstream manifest each thread fetches. So sweep on first ask and
/// memoize rather than trying to enumerate up front.
pub(super) struct ChannelIndexCache {
    target_platform: Arch,
    // ponytail: one lock over the whole map, held across the sweep, so sweeps
    // of *different* channels don't overlap. Deliberate — it also means a
    // channel is never swept twice concurrently, and the totals are small
    // (~23 channels for ros-recipes, product channels hold 1-2 packages and
    // sweep in ~0.5s). Go per-channel locks if the channel count grows enough
    // that serialised cold sweeps start to show.
    swept: Mutex<HashMap<RemoteChannel, Arc<ChannelIndex>>>,
}

impl ChannelIndexCache {
    pub(super) fn new(target_platform: Arch) -> Self {
        Self {
            target_platform,
            swept: Mutex::new(HashMap::new()),
        }
    }

    /// The snapshot for `channel`, sweeping it if this is the first ask.
    ///
    /// Only a [`RemoteChannel`] can be asked: a snapshot is only sound for a
    /// channel the job cannot mutate while holding it. Local channels gain
    /// packages as this job publishes into them and go through
    /// [`version_published`] instead.
    pub(super) fn get(&self, channel: &RemoteChannel) -> anyhow::Result<Arc<ChannelIndex>> {
        let mut swept = self.swept.lock().expect("channel index cache poisoned");
        if let Some(index) = swept.get(channel) {
            return Ok(Arc::clone(index));
        }
        let index = Arc::new(ChannelIndex::sweep(channel, self.target_platform)?);
        swept.insert(channel.clone(), Arc::clone(&index));
        Ok(index)
    }
}

/// Whether *any* build of `name == version` exists in `channel` for
/// `target_platform`, asked live.
///
/// Takes a [`LocalChannel`] because that is the only kind that has to be asked
/// live: those gain packages as the job publishes into them, so a snapshot
/// would go stale mid-loop. Their repodata is small and uncontended, so the
/// per-package search is cheap here. Remote channels go through
/// [`ChannelIndexCache`] instead.
pub(super) fn version_published(
    name: &PackageName,
    version: &Version,
    channel: &LocalChannel,
    target_platform: Arch,
) -> anyhow::Result<bool> {
    let spec = format!("{name}=={version}");
    let Some(parsed) = search_channel(&spec, &channel.to_string(), target_platform)? else {
        return Ok(false);
    };
    let version = version.to_string();
    Ok(parsed
        .values()
        .flatten()
        .any(|r| r.name == name.as_str() && r.version == version))
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
