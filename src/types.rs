//! Shared newtypes and enums used across the CLI and the shared layer.
//!
//! Everything here is a *parse* type: the only way to obtain one is through a
//! constructor that has already discharged the invariant, so no code
//! downstream re-checks a string it was handed. Where a value is deserialized
//! from a manifest, the constructor runs inside `Deserialize`, which puts the
//! rejection at the file, not at the first use.

use std::borrow::Borrow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Arch {
    #[serde(rename = "linux-64")]
    Linux64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DeepstreamVersion {
    #[serde(rename = "7.1")]
    V7_1,
    #[serde(rename = "8.0")]
    V8_0,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum RunnerSize {
    #[serde(rename = "4cpu")]
    #[default]
    Cpu4,
    #[serde(rename = "8cpu")]
    Cpu8,
    #[serde(rename = "16cpu")]
    Cpu16,
    #[serde(rename = "32cpu")]
    Cpu32,
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linux64 => "linux-64",
            Self::LinuxAarch64 => "linux-aarch64",
        })
    }
}

impl FromStr for Arch {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux-64" => Ok(Self::Linux64),
            "linux-aarch64" => Ok(Self::LinuxAarch64),
            other => anyhow::bail!("unknown arch {other:?}; expected linux-64 or linux-aarch64"),
        }
    }
}

impl fmt::Display for DeepstreamVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::V7_1 => "7.1",
            Self::V8_0 => "8.0",
        })
    }
}

impl FromStr for DeepstreamVersion {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "7.1" => Ok(Self::V7_1),
            "8.0" => Ok(Self::V8_0),
            other => anyhow::bail!("unknown DeepStream version {other:?}; expected 7.1 or 8.0"),
        }
    }
}

// ---------------------------------------------------------------------------
// Package name
// ---------------------------------------------------------------------------

/// A conda/pixi package name.
///
/// **The rule:** non-empty, first character ASCII alphanumeric, every
/// character ASCII alphanumeric or one of `-`, `_`, `.`.
///
/// That set is chosen from what the *consumers* of a name require, not from
/// how the names in this fleet happen to look, because a name that satisfies
/// the rule is safe at every site one reaches:
///
/// * spliced into a `pixi search` match spec (`<name>==<version>`) — so no
///   `=`, `<`, `>`, `*`, `,`, `[`, or whitespace;
/// * written back as a bare YAML scalar (`name: <name>`, `- <name>:`) — so no
///   `:`, `#`, quote characters, and no leading `-`;
/// * used as an argv element and as the head of a synthesized `.conda`
///   filename for routing.
///
/// The rule is deliberately *permissive*, because this same type carries names
/// the tool does not control: dependency **keys** copied out of upstream
/// `pixi.toml` files and solved environments. Rejecting a key a manifest
/// legitimately declares would turn a parse error into a build outage, so the
/// rule only forbids what would actually break a consumer above. Concretely:
///
/// * uppercase is allowed even though conda names are lowercase by policy, and
///   case is preserved rather than folded, since the name is compared against
///   channel records verbatim;
/// * a leading `_` is allowed, because conda-forge really ships
///   `_libgcc_mutex` and `_openmp_mutex`, and conda virtual packages surface as
///   `__linux` / `__glibc` / `__cuda`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PackageName(String);

impl PackageName {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        let mut chars = s.chars();
        let ok = match chars.next() {
            None => false,
            Some(first) => {
                (first.is_ascii_alphanumeric() || first == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            }
        };
        anyhow::ensure!(
            ok,
            "not a package name: {s:?} (expected alphanumeric, `-`, `_` or `.`, \
             starting with a letter, digit or `_`)"
        );
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PackageName {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Lets a `BTreeMap<PackageName, _>` / `BTreeSet<PackageName>` be probed with a
/// plain `&str`. Sound because the derived `Ord`/`Eq`/`Hash` delegate to the
/// inner `String`, which agrees with `str`'s.
impl Borrow<str> for PackageName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// A release version of a package this repo publishes.
///
/// Ordered by [`semver::Version`], which is what the release pipeline actually
/// produces: semantic-release emits `X.Y.Z` and `X.Y.Z-alpha.N`. Ordering by
/// semver buys the correct total order for free — it compares numeric
/// prerelease identifiers numerically, so `alpha.2 < alpha.10`, which the
/// hand-rolled sort key this replaces got backwards.
///
/// A shorter `X` or `X.Y` is accepted too and filled out to a triple for
/// ordering, because pixi/conda allow it and hand-written manifests use it. The
/// text as written is kept alongside, and `Display` re-emits *that*, so touching
/// a manifest that said `1.0` leaves it saying `1.0`. Two versions that fill out
/// to the same triple are equal and hash alike; the text is presentation only
/// and deliberately not part of identity.
///
/// This is still narrower than a *conda* version, which also admits epochs
/// (`1!1.0`) and `post`/`dev` segments. Those are rejected, as is `+build`
/// metadata: the semver *spec* says build metadata is ignored when comparing,
/// but the `semver` crate derives `Ord`/`PartialEq` across it, so to this
/// program `1.0.0+a` and `1.0.0+b` would be neither equal nor unordered.
/// Refusing it beats either silently dropping it or ordering against the spec.
///
/// Versions arriving from a channel are not parsed into this type at all — see
/// `build_recipes::channel::ChannelIndex`.
#[derive(Debug, Clone)]
pub struct Version {
    /// As written by whoever produced it. Never re-derived, so a round trip
    /// through this type cannot rewrite a manifest that was already fine.
    text: String,
    /// The comparison key: `text` filled out to a semver triple.
    parsed: semver::Version,
}

impl Version {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        // `semver::Comparator` is the crate's own lenient parser: it makes the
        // minor and patch components optional, which is exactly the widening
        // we want, without a second version grammar to maintain. It is a
        // *requirement* parser though, so two things it accepts have to be
        // refused here.
        //
        // Build metadata first, because `Comparator` has no field for it and
        // would drop it silently — and to this program build metadata is
        // ordering- and equality-significant (see the type doc), so dropping
        // it would fuse two distinct artifacts into one value.
        anyhow::ensure!(
            !s.contains('+'),
            "not a semantic version: {s:?} (build metadata is not accepted here; \
             it would have to be dropped, and it is not droppable)"
        );
        // Then anything that is not a plain version. `Comparator` happily eats
        // a leading `>=`, `~`, `^`, `<` or `=`, and reading `>=1.0` as the
        // version `1.0.0` would quietly answer a question nobody asked. A
        // version starts with a digit; a requirement (or a `v` prefix) does
        // not.
        anyhow::ensure!(
            s.starts_with(|c: char| c.is_ascii_digit()),
            "not a semantic version: {s:?} (expected a version like 1, 1.2 or 1.2.3 — \
             a requirement or a prefix is not a version)"
        );
        let c: semver::Comparator = s
            .parse()
            .map_err(|e| anyhow::anyhow!("not a semantic version: {s:?} ({e})"))?;
        // And a wildcard, which starts with a digit and so slips past the check
        // above: `2.5.*` parses as the wildcard comparator `2.5`.
        anyhow::ensure!(
            c.op == semver::Op::Caret,
            "not a semantic version: {s:?} (expected a version like 1, 1.2 or 1.2.3 — \
             a wildcard is not a version)"
        );
        Ok(Self {
            text: s.to_string(),
            parsed: semver::Version {
                major: c.major,
                minor: c.minor.unwrap_or(0),
                patch: c.patch.unwrap_or(0),
                pre: c.pre,
                build: semver::BuildMetadata::EMPTY,
            },
        })
    }

    pub fn major(&self) -> u64 {
        self.parsed.major
    }

    /// Whether the text as written spells out all three components, rather
    /// than leaning on this type filling them out. Callers that write a *pin*
    /// need this: `==1.2` means something wider than `==1.2.0` to the solver,
    /// so it must not be read as an exact pin just because it parses here.
    pub fn is_explicit_triple(&self) -> bool {
        // Build metadata cannot be present, so `-` bounds the numeric core.
        let core = &self.text[..self.text.find('-').unwrap_or(self.text.len())];
        core.matches('.').count() == 2
    }

    /// The derived sibling pin `>=<self>,<<major+1>`: floor at this exact
    /// version, cap at the next major. Prerelease floors are fine —
    /// `>=1.24.0-alpha.2,<2` admits the prerelease and everything after it.
    pub fn range_pin(&self) -> String {
        format!(">={self},<{}", self.major() + 1)
    }
}

/// Identity and ordering are the padded semver triple, never the source text:
/// `1.0` and `1.0.0` name the same release and must not sort or hash apart.
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parsed == other.parsed
    }
}

impl Eq for Version {}

impl std::hash::Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.parsed.hash(state);
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.parsed.cmp(&other.parsed)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.text)
    }
}

// ---------------------------------------------------------------------------
// Git SHA
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha40(String);

impl Sha40 {
    pub fn new(s: impl Into<String>) -> anyhow::Result<Self> {
        let s = s.into();
        if s.len() != 40 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            anyhow::bail!("not a 40-char lowercase hex sha: {s:?}");
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha40 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Sha40 {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<'de> Deserialize<'de> for Sha40 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

/// The name of a vinca recipe, as written in the vinca config.
///
/// Unlike everything else in this module this is a **label, not a parse type**:
/// it carries no invariant, its `FromStr` is infallible, and its `Deserialize`
/// accepts any string. Holding one proves only that some string was labelled a
/// recipe name. Do not assume it has been validated, and do not add a rule here
/// without checking every producer — the vinca config is not ours.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecipeName(String);

impl RecipeName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecipeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// GitHub repository URL
// ---------------------------------------------------------------------------

/// A GitHub repository, identified by owner and name.
///
/// Fields are private and the only constructor validates, so every rendering a
/// caller might want is a method here rather than string surgery at the call
/// site: the `.git` suffix, the ssh↔https normalization and the compare-URL
/// shape each have exactly one definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GithubRepoUrl {
    owner: String,
    repo: String,
}

impl GithubRepoUrl {
    /// Parse an `https://github.com/<owner>/<repo>[.git][/]` URL.
    pub fn parse(url: &str) -> anyhow::Result<Self> {
        let rest = url
            .strip_prefix("https://github.com/")
            .ok_or_else(|| anyhow::anyhow!("not a GitHub https URL: {url:?}"))?;
        Self::from_owner_repo(rest, url)
    }

    /// Parse any spelling git writes into `remote.<name>.url`: the https URL
    /// above, the scp-style `git@github.com:<owner>/<repo>[.git]`, or the full
    /// `ssh://[git@]github.com/<owner>/<repo>[.git]`.
    ///
    /// All three occur in real checkouts — the ssh forms depend on how the
    /// clone was made and on any `url.*.insteadOf` rewrite in effect — and the
    /// caller only has whatever git reports, so refusing one would fail a
    /// release for a reason that has nothing to do with the release.
    pub fn parse_remote(url: &str) -> anyhow::Result<Self> {
        const SSH_PREFIXES: [&str; 3] = [
            "git@github.com:",
            "ssh://git@github.com/",
            "ssh://github.com/",
        ];
        match SSH_PREFIXES.iter().find_map(|p| url.strip_prefix(p)) {
            Some(rest) => Self::from_owner_repo(rest, url),
            None => Self::parse(url),
        }
    }

    /// The shared tail of both parsers: `<owner>/<repo>` with the optional
    /// `.git` and trailing slash removed. `url` is only for the error message.
    fn from_owner_repo(rest: &str, url: &str) -> anyhow::Result<Self> {
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let (owner, repo) = rest
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("not a GitHub repo URL: {url:?}"))?;
        anyhow::ensure!(
            !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
            "not a GitHub repo URL: {url:?}"
        );
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The repository's short name — the `<repo>` of `<owner>/<repo>`.
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// `<owner>/<repo>`, the slug every `gh` subcommand and API path uses.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// Browsable URL, no `.git` suffix. Also what `git` accepts as a remote.
    pub fn https_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }

    /// Clone URL with the `.git` suffix, the form the recipe manifests record.
    pub fn git_url(&self) -> String {
        format!("{}.git", self.https_url())
    }

    /// GitHub's two-dot compare page between two refs of this repository.
    pub fn compare_url(&self, old: &str, new: &str) -> String {
        format!("{}/compare/{old}...{new}", self.https_url())
    }
}

impl fmt::Display for GithubRepoUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.https_url())
    }
}

impl FromStr for GithubRepoUrl {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl<'de> Deserialize<'de> for GithubRepoUrl {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for GithubRepoUrl {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.https_url())
    }
}

// ---------------------------------------------------------------------------
// Channels
// ---------------------------------------------------------------------------

/// A conda channel served over the network.
///
/// The capability that matters lives on this type rather than on
/// [`ChannelUrl`]: a remote channel cannot change while a build job runs
/// (builds publish into a local output channel and are drained to the real
/// ones afterwards), so its index may be swept once and cached. That is why
/// `ChannelIndexCache::get` takes a `&RemoteChannel` — handing it a local
/// channel is a compile error rather than a rule stated in a comment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteChannel(Url);

impl RemoteChannel {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let url = Url::parse(s).map_err(|e| anyhow::anyhow!("not a channel URL: {s:?} ({e})"))?;
        anyhow::ensure!(
            url.scheme() != "file",
            "{s:?} is a local channel; remote channel URLs are swept once and \
             cached, which is only sound for a channel the build cannot mutate"
        );
        Ok(Self(url))
    }

    /// The channel of the same host and parent path named `channel`.
    ///
    /// Routing names destination channels relative to the default one
    /// (`.../general` → `.../gama`), so this replaces the last path segment.
    pub fn sibling(&self, channel: &str) -> Self {
        let mut url = self.0.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().pop().push(channel);
        }
        Self(url)
    }
}

impl fmt::Display for RemoteChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl FromStr for RemoteChannel {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// A conda channel that is a directory on this machine.
///
/// The counterpart of [`RemoteChannel`]: a build publishes into one of these
/// as it goes, so a snapshot of it goes stale mid-loop and it has to be
/// queried live.
///
/// Holds the directory rather than a `Url` because the `file://` rendering is
/// consumed by `pixi`, and building it by hand keeps a path with characters a
/// URL would percent-encode byte-identical to what previous releases emitted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalChannel(PathBuf);

impl LocalChannel {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self(dir.into())
    }
}

impl fmt::Display for LocalChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "file://{}", self.0.display())
    }
}

/// Either kind of channel, for the places that only forward a channel on
/// (a solver argument, a `channels` array) and do not care which it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelUrl {
    Remote(RemoteChannel),
    Local(LocalChannel),
}

impl ChannelUrl {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.strip_prefix("file://") {
            Some(dir) => Ok(Self::Local(LocalChannel::new(dir))),
            None => Ok(Self::Remote(RemoteChannel::parse(s)?)),
        }
    }
}

impl fmt::Display for ChannelUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Remote(c) => fmt::Display::fmt(c, f),
            Self::Local(c) => fmt::Display::fmt(c, f),
        }
    }
}

impl FromStr for ChannelUrl {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl From<LocalChannel> for ChannelUrl {
    fn from(c: LocalChannel) -> Self {
        Self::Local(c)
    }
}

impl From<RemoteChannel> for ChannelUrl {
    fn from(c: RemoteChannel) -> Self {
        Self::Remote(c)
    }
}

// ---------------------------------------------------------------------------
// pixi-native manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PixiNativeEntry {
    pub name: PackageName,
    pub url: GithubRepoUrl,
    pub rev: Sha40,
    pub subdir: Option<PathBuf>,
    pub runner_size: RunnerSize,
}

impl PixiNativeEntry {
    /// The entry's directory inside its repo checkout: its `subdir`, or the
    /// checkout root when it has none.
    pub fn subdir_or_root(&self) -> &Path {
        self.subdir.as_deref().unwrap_or(Path::new("."))
    }
}

#[derive(Deserialize)]
struct PixiNativeEntryRaw {
    name: PackageName,
    url: GithubRepoUrl,
    #[serde(default)]
    rev: Option<Sha40>,
    // Kept solely to produce a helpful migration-pointer error when `ref:` appears.
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    subdir: Option<PathBuf>,
    #[serde(default, rename = "runner-size")]
    runner_size: Option<RunnerSize>,
}

impl TryFrom<PixiNativeEntryRaw> for PixiNativeEntry {
    type Error = anyhow::Error;
    fn try_from(raw: PixiNativeEntryRaw) -> Result<Self, Self::Error> {
        if raw.git_ref.is_some() {
            anyhow::bail!(
                "entry {:?}: `ref:` is no longer supported; use `rev:` with a 40-char SHA. \
                 See ros-recipes/scripts/ for a one-shot migrator.",
                raw.name.as_str()
            );
        }
        let rev = raw.rev.ok_or_else(|| {
            anyhow::anyhow!(
                "entry {:?}: missing required `rev:` (40-char SHA)",
                raw.name.as_str()
            )
        })?;
        Ok(Self {
            name: raw.name,
            url: raw.url,
            rev,
            subdir: raw.subdir,
            runner_size: raw.runner_size.unwrap_or_default(),
        })
    }
}

#[derive(Deserialize)]
struct PixiNativeManifestRaw {
    #[serde(default)]
    rebuild_epoch: u64,
    packages: Vec<PixiNativeEntryRaw>,
}

#[derive(Debug, Clone)]
pub struct PixiNativeManifest {
    /// Global rebuild salt. Added to each package's upstream `build-number`
    /// when computing the effective build number used for the channel-skip
    /// check and the published artifact. Bump to force a fleet-wide rebuild
    /// (e.g. after a ros2-distro-mutex / fastdds-mutex bump).
    pub rebuild_epoch: u64,
    pub packages: Vec<PixiNativeEntry>,
}

impl PixiNativeManifest {
    pub fn from_yaml_str(yaml: &str) -> anyhow::Result<Self> {
        let raw: PixiNativeManifestRaw = serde_yaml_ng::from_str(yaml)?;
        let packages = raw
            .packages
            .into_iter()
            .map(PixiNativeEntry::try_from)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            rebuild_epoch: raw.rebuild_epoch,
            packages,
        })
    }

    /// True if the manifest lists an entry named `name`.
    ///
    /// Deliberately parsed through [`PixiNativeNames`] rather than
    /// [`Self::from_yaml_str`]: this answers "is this package routed
    /// pixi-native?" for a package we are about to rewrite, and it must not
    /// fail because some *other* entry in the file is malformed (an unmigrated
    /// `ref:`, a URL the buildfarm hasn't taught itself to parse). The file's
    /// own structure still has to be valid YAML.
    pub fn has_entry(yaml: &str, name: &PackageName) -> anyhow::Result<bool> {
        let names: PixiNativeNames = serde_yaml_ng::from_str(yaml)?;
        Ok(names.packages.iter().any(|p| &p.name == name))
    }
}

/// Entry names only — see [`PixiNativeManifest::has_entry`].
#[derive(Deserialize)]
struct PixiNativeNames {
    packages: Vec<PixiNativeName>,
}

#[derive(Deserialize)]
struct PixiNativeName {
    name: PackageName,
}

impl FromStr for RecipeName {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl fmt::Display for RunnerSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cpu4 => "4cpu",
            Self::Cpu8 => "8cpu",
            Self::Cpu16 => "16cpu",
            Self::Cpu32 => "32cpu",
        })
    }
}

impl FromStr for RunnerSize {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4cpu" => Ok(Self::Cpu4),
            "8cpu" => Ok(Self::Cpu8),
            "16cpu" => Ok(Self::Cpu16),
            "32cpu" => Ok(Self::Cpu32),
            other => anyhow::bail!(
                "unknown runner size {other:?}; expected one of 4cpu/8cpu/16cpu/32cpu"
            ),
        }
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
