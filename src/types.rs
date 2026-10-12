//! Shared newtypes and enums used across the CLI and the shared layer.
//!
//! Everything here is a *parse* type: the only way to obtain one is through a
//! constructor that has already discharged the invariant, so no code
//! downstream re-checks a string it was handed. Where a value is deserialized
//! from a manifest, the constructor runs inside `Deserialize`, which puts the
//! rejection at the file, not at the first use.

use std::borrow::Borrow;
use std::fmt;
use std::path::Path;
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

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Linux64 => "linux-64",
            Self::LinuxAarch64 => "linux-aarch64",
        })
    }
}

impl FromStr for Arch {
    type Err = color_eyre::eyre::Report;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux-64" => Ok(Self::Linux64),
            "linux-aarch64" => Ok(Self::LinuxAarch64),
            other => color_eyre::eyre::bail!(
                "unknown arch {other:?}; expected linux-64 or linux-aarch64"
            ),
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
/// A name satisfying the rule is safe at every site it reaches:
///
/// * spliced into a `pixi search` match spec (`<name>==<version>`) — so no
///   `=`, `<`, `>`, `*`, `,`, `[`, or whitespace;
/// * written back as a bare YAML scalar (`name: <name>`, `- <name>:`) — so no
///   `:`, `#`, quote characters, and no leading `-`;
/// * used as an argv element.
///
/// The rule is *permissive* because this type also carries names the tool
/// does not control — dependency **keys** copied out of upstream `pixi.toml`
/// files and solved environments — so it only forbids what would break a
/// site above. Concretely:
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
    pub fn new(s: impl Into<String>) -> color_eyre::eyre::Result<Self> {
        let s = s.into();
        let mut chars = s.chars();
        let ok = chars.next().is_some_and(|first| {
            (first.is_ascii_alphanumeric() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
        color_eyre::eyre::ensure!(
            ok,
            "not a package name: {s:?} (expected alphanumeric, `-`, `_` or `.`, \
             starting with a letter, digit or `_`)"
        );
        Ok(Self(s))
    }

    #[must_use]
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
    type Err = color_eyre::eyre::Report;
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
// Release tag
// ---------------------------------------------------------------------------

/// The git tag a release of one package creates: `<package>@<version>`. The
/// semantic-release `--tag-format` and the tag recorded in recipes both come
/// from here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTag {
    package: PackageName,
    version: Version,
}

impl ReleaseTag {
    #[must_use]
    pub fn new(package: &PackageName, version: &Version) -> Self {
        Self {
            package: package.clone(),
            version: version.clone(),
        }
    }

    /// The semantic-release tag format for `package`, or for whichever package
    /// multi-semantic-release is releasing when `None`.
    #[must_use]
    pub fn format(package: Option<&PackageName>) -> String {
        package.map_or_else(
            || "${name}@${version}".to_string(),
            |package| format!("{package}@${{version}}"),
        )
    }
}

impl fmt::Display for ReleaseTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.package, self.version)
    }
}

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// A release version of a package this repo publishes.
///
/// Ordered by [`semver::Version`], which is what the release pipeline
/// produces: semantic-release emits `X.Y.Z` and `X.Y.Z-alpha.N`, and semver
/// compares numeric prerelease identifiers numerically (`alpha.2 < alpha.10`).
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
/// Versions arriving from a channel are not parsed into this type at all.
#[derive(Debug, Clone)]
pub struct Version {
    /// As written by whoever produced it. Never re-derived, so a round trip
    /// through this type cannot rewrite a manifest that was already fine.
    text: String,
    /// The comparison key: `text` filled out to a semver triple.
    parsed: semver::Version,
}

impl Version {
    pub fn parse(s: &str) -> color_eyre::eyre::Result<Self> {
        // `semver::Comparator` is the crate's own lenient parser: it makes
        // the minor and patch components optional. It is a *requirement*
        // parser though, so some of what it accepts must be refused here.
        //
        // Build metadata first, because `Comparator` has no field for it and
        // would drop it silently — and to this program build metadata is
        // ordering- and equality-significant (see the type doc), so dropping
        // it would fuse two distinct artifacts into one value.
        color_eyre::eyre::ensure!(
            !s.contains('+'),
            "not a semantic version: {s:?} (build metadata is not accepted here; \
             it would have to be dropped, and it is not droppable)"
        );
        // Then anything that is not a plain version. `Comparator` happily eats
        // a leading `>=`, `~`, `^`, `<` or `=`, and reading `>=1.0` as the
        // version `1.0.0` would quietly answer a question nobody asked. A
        // version starts with a digit; a requirement (or a `v` prefix) does
        // not.
        color_eyre::eyre::ensure!(
            s.starts_with(|c: char| c.is_ascii_digit()),
            "not a semantic version: {s:?} (expected a version like 1, 1.2 or 1.2.3 — \
             a requirement or a prefix is not a version)"
        );
        let c: semver::Comparator = s
            .parse()
            .map_err(|e| color_eyre::eyre::eyre!("not a semantic version: {s:?} ({e})"))?;
        // And a wildcard, which starts with a digit and so slips past the check
        // above: `2.5.*` parses as the wildcard comparator `2.5`.
        color_eyre::eyre::ensure!(
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

    #[must_use]
    pub const fn major(&self) -> u64 {
        self.parsed.major
    }

    /// The derived sibling pin `>=<self>,<<major+1>`: floor at this exact
    /// version, cap at the next major. Prerelease floors are fine —
    /// `>=1.24.0-alpha.2,<2` admits the prerelease and everything after it.
    #[must_use]
    pub fn range_pin(&self) -> String {
        format!(">={self},<{}", self.major().saturating_add(1))
    }

    /// The exact pin `==<self>`, for lockstep-coupled siblings.
    #[must_use]
    pub fn exact_pin(&self) -> String {
        format!("=={self}")
    }
}

/// Version-pin form for a rewritten sibling `path =` dep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SiblingPinStyle {
    /// `>=<version>,<major+1>` — published consumers accept future sibling
    /// releases within the major without a re-release.
    #[default]
    Range,
    /// `==<version>` — lockstep coupling: the consumer only ever runs against
    /// the sibling version it was built with.
    Exact,
}

impl SiblingPinStyle {
    #[must_use]
    pub fn pin(self, version: &Version) -> String {
        match self {
            Self::Range => version.range_pin(),
            Self::Exact => version.exact_pin(),
        }
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
    type Err = color_eyre::eyre::Report;
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
    pub fn new(s: impl Into<String>) -> color_eyre::eyre::Result<Self> {
        let s = s.into();
        if s.len() != 40 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            color_eyre::eyre::bail!("not a 40-char lowercase hex sha: {s:?}");
        }
        Ok(Self(s))
    }

    #[must_use]
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
    type Err = color_eyre::eyre::Report;
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

// ---------------------------------------------------------------------------
// GitHub repository URL
// ---------------------------------------------------------------------------

/// A GitHub repository, identified by owner and name. The `.git` suffix, the
/// ssh↔https normalization and the compare-URL shape each have exactly one
/// definition here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GithubRepoUrl {
    owner: String,
    repo: String,
}

impl GithubRepoUrl {
    /// Parse an `https://github.com/<owner>/<repo>[.git][/]` URL.
    pub fn parse(url: &str) -> color_eyre::eyre::Result<Self> {
        let rest = url
            .strip_prefix("https://github.com/")
            .ok_or_else(|| color_eyre::eyre::eyre!("not a GitHub https URL: {url:?}"))?;
        Self::from_owner_repo(rest, url)
    }

    /// Parse any spelling git writes into `remote.<name>.url`: the https URL
    /// above, the scp-style `git@github.com:<owner>/<repo>[.git]`, or the full
    /// `ssh://[git@]github.com/<owner>/<repo>[.git]`. All three occur in real
    /// checkouts, and the caller only has whatever git reports.
    pub fn parse_remote(url: &str) -> color_eyre::eyre::Result<Self> {
        const SSH_PREFIXES: [&str; 3] = [
            "git@github.com:",
            "ssh://git@github.com/",
            "ssh://github.com/",
        ];
        SSH_PREFIXES
            .iter()
            .find_map(|p| url.strip_prefix(p))
            .map_or_else(|| Self::parse(url), |rest| Self::from_owner_repo(rest, url))
    }

    /// The shared tail of both parsers: `<owner>/<repo>` with the optional
    /// `.git` and trailing slash removed. `url` is only for the error message.
    fn from_owner_repo(rest: &str, url: &str) -> color_eyre::eyre::Result<Self> {
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        let (owner, repo) = rest
            .split_once('/')
            .ok_or_else(|| color_eyre::eyre::eyre!("not a GitHub repo URL: {url:?}"))?;
        color_eyre::eyre::ensure!(
            !owner.is_empty() && !repo.is_empty() && !repo.contains('/'),
            "not a GitHub repo URL: {url:?}"
        );
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The repository's short name — the `<repo>` of `<owner>/<repo>`.
    #[must_use]
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// `<owner>/<repo>`, the slug every `gh` subcommand and API path uses.
    #[must_use]
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    /// Browsable URL, no `.git` suffix. Also what `git` accepts as a remote.
    #[must_use]
    pub fn https_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }

    /// Clone URL with the `.git` suffix, the form the recipe manifests record.
    #[must_use]
    pub fn git_url(&self) -> String {
        format!("{}.git", self.https_url())
    }

    /// Whether both name the same repository; GitHub owner and repo names are
    /// case-insensitive.
    #[must_use]
    pub fn same_repo(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.repo.eq_ignore_ascii_case(&other.repo)
    }

    /// GitHub's two-dot compare page between two refs of this repository.
    #[must_use]
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
    type Err = color_eyre::eyre::Report;
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

/// A conda channel that is a directory on this machine, rendered as a
/// `file://` URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalChannel(Url);

impl LocalChannel {
    /// An empty channel at `dir`, created or emptied on disk.
    pub fn fresh(dir: &Path) -> color_eyre::eyre::Result<Self> {
        use color_eyre::eyre::WrapErr;
        if dir.exists() {
            std::fs::remove_dir_all(dir).with_context(|| format!("clearing {}", dir.display()))?;
        }
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let abs =
            std::fs::canonicalize(dir).with_context(|| format!("resolving {}", dir.display()))?;
        Url::from_directory_path(&abs).map(Self).map_err(|()| {
            color_eyre::eyre::eyre!("{} is not a valid channel directory", abs.display())
        })
    }

    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.0
    }
}

impl fmt::Display for LocalChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

// ---------------------------------------------------------------------------
// pixi-native manifest
// ---------------------------------------------------------------------------

/// True if the pixi-native manifest lists an entry named `name`.
///
/// Parsed through [`PixiNativeNames`]: the answer must not fail because some
/// *other* entry in the file is malformed. The file's own structure still has
/// to be valid YAML.
pub fn pixi_native_has_entry(yaml: &str, name: &PackageName) -> color_eyre::eyre::Result<bool> {
    let names: PixiNativeNames = serde_yaml_ng::from_str(yaml)?;
    Ok(names.packages.iter().any(|p| &p.name == name))
}

#[derive(Deserialize)]
struct PixiNativeNames {
    packages: Vec<PixiNativeName>,
}

#[derive(Deserialize)]
struct PixiNativeName {
    name: PackageName,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
