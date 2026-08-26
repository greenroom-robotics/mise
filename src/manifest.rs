//! `pixi.toml`: one reader, one writer.
//!
//! Two views live here, deliberately, because they answer different questions:
//!
//! * **Read** — [`Manifest`] / [`PackageManifest`], a serde model. Use it for
//!   every question *about* a manifest: package identity, build settings,
//!   platform support, dependency tables. It is lossy (comments, key order and
//!   spacing are gone) and it cannot write.
//! * **Write** — the `toml_edit` functions ([`set_package_version`],
//!   [`set_build_number`], [`resolve_path_deps`], [`prepend_channels`]). Use
//!   them for every mutation: a manifest we rewrite is either committed by
//!   semantic-release or handed to `pixi build`, and in both cases the diff has
//!   to be limited to the key we touched. Never round-trip a manifest through
//!   the serde model to change it.
//!
//! [`Package`] is the two joined at the point of discovery: a manifest parsed
//! once, carrying the directory that owns it, so no downstream command re-reads
//! the file or re-derives the directory.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::consts::PIXI_TOML;
use crate::types::{Arch, ChannelUrl, PackageName, SiblingPinStyle, Version};

/// The dependency tables a pixi package can declare, in scan order.
///
/// One list because the readers and the writer have to agree: a table missing
/// from a reader is a sibling edge nobody sees (a release ordered wrong), and a
/// table missing from the writer is a `path =` dep that reaches the build farm
/// unresolved. `[dependencies]` holds only the self-as-workspace-member idiom
/// today but is scanned for safety.
pub const DEP_TABLES: &[&[&str]] = &[
    &["dependencies"],
    &["package", "run-dependencies"],
    &["package", "host-dependencies"],
    &["package", "build-dependencies"],
];

// ---------------------------------------------------------------------------
// Read view
// ---------------------------------------------------------------------------

/// A parsed `pixi.toml`.
///
/// The two variants are the two kinds of manifest this repo's tooling meets,
/// so no caller has to probe for a `[package]` table itself.
#[derive(Debug)]
pub enum Manifest {
    /// No `[package]` table.
    ///
    /// A workspace-only manifest is a dev/test environment for something this
    /// repo does not publish — e.g. a DeepStream-linked package whose artifact
    /// comes from a hand-authored recipe in ros-recipes, but which still wants
    /// a pixi env for colcon and the ROS dev tools. It has no version to
    /// release and nothing for `pixi build` to build.
    WorkspaceOnly,
    /// Declares a `[package]` table: a releasable, buildable package.
    Package(PackageManifest),
}

/// The read view of a manifest that declares a `[package]`.
#[derive(Debug)]
pub struct PackageManifest {
    package: PackageSection,
    workspace: Option<Workspace>,
    /// Every dependency entry across [`DEP_TABLES`], in that order.
    deps: Vec<Dep>,
}

/// A package's name and version together, which is what every release path
/// needs. Guaranteed present on any [`PackageManifest`] value: both keys are
/// required to deserialize `[package]`, so a manifest that parses into one
/// always has an identity. A `[package]` that fails on those keys never becomes
/// a `PackageManifest` — see [`Candidate::UnreadableIdentity`].
#[derive(Debug, PartialEq, Eq)]
pub struct PackageIdentity {
    pub name: PackageName,
    pub version: Version,
}

/// One entry from a dependency table: the dep *key* — which is the channel
/// artifact name, not necessarily the sibling's `package.name` — and its
/// requirement value.
#[derive(Debug)]
pub struct Dep {
    pub name: PackageName,
    value: toml::Value,
}

#[derive(Debug, Deserialize)]
struct ManifestRaw {
    /// Left as a raw value so the `[package]` table is deserialized separately
    /// and its errors can be attributed to that table by name.
    #[serde(default)]
    package: Option<toml::Value>,
    #[serde(default)]
    workspace: Option<Workspace>,
}

/// Both identity keys are required: a `[package]` table that does not name and
/// version itself is rejected here, once, rather than at every use.
#[derive(Debug, Deserialize)]
struct PackageSection {
    name: PackageName,
    version: Version,
    #[serde(default)]
    build: Option<BuildSection>,
}

#[derive(Debug, Deserialize)]
struct BuildSection {
    #[serde(default)]
    backend: Option<BuildBackend>,
    #[serde(default)]
    config: Option<BuildConfig>,
}

#[derive(Debug, Deserialize)]
struct BuildBackend {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct BuildConfig {
    /// Defaults to 0 when omitted from the manifest.
    #[serde(default, rename = "build-number")]
    build_number: u64,
    #[serde(default, rename = "build-type")]
    build_type: String,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    #[serde(default)]
    platforms: Vec<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let value: toml::Value = toml::from_str(text)?;
        let deps = collect_deps(&value)?;
        let raw: ManifestRaw = value.try_into()?;
        Ok(match raw.package {
            None => Manifest::WorkspaceOnly,
            Some(package) => Manifest::Package(PackageManifest {
                package: package.try_into().context("[package]")?,
                workspace: raw.workspace,
                deps,
            }),
        })
    }

    pub fn read(manifest_path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", manifest_path.display()))
    }
}

/// Every dependency entry in [`DEP_TABLES`] order. Values are cloned out of the
/// document so the read view owns them; dep tables are a handful of entries.
fn collect_deps(value: &toml::Value) -> Result<Vec<Dep>> {
    let mut out = Vec::new();
    for table_path in DEP_TABLES {
        let mut node = value;
        let mut found = true;
        for seg in *table_path {
            match node.get(seg) {
                Some(next) => node = next,
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
        for (name, value) in table {
            out.push(Dep {
                name: PackageName::new(name.clone())
                    .with_context(|| format!("dependency key in [{}]", table_path.join(".")))?,
                value: value.clone(),
            });
        }
    }
    Ok(out)
}

impl Dep {
    /// The `path = "..."` of a local dep, relative to the consumer's directory.
    ///
    /// Callers must exclude the self-as-workspace-member idiom themselves, and
    /// they do it two different ways on purpose: lexically (`path == "."`) when
    /// only the manifest is in hand, and by resolved target when the caller
    /// knows the package's own directory (which also catches `../<self>`).
    pub fn path(&self) -> Option<&str> {
        self.value.get("path").and_then(toml::Value::as_str)
    }
}

/// The version of an exact `==X.Y.Z` pin, else `None`. Exact means `==`
/// followed by a single concrete semantic version. Ranges (`>=`, `<`),
/// wildcards (`==1.2.*`) and `+build` metadata are not pins and [`Version`]
/// already rejects all of them. The three-component check is the one exclusion
/// that has to be explicit, because `Version` is wider than a pin here: a
/// two-component `==1.2` parses, but it is a range to the solver.
fn exact_pin_version(s: &str) -> Option<Version> {
    Version::parse(s.trim().strip_prefix("==")?)
        .ok()
        .filter(Version::is_explicit_triple)
}

impl PackageManifest {
    pub fn parse(text: &str) -> Result<Self> {
        match Manifest::parse(text)? {
            Manifest::Package(m) => Ok(m),
            Manifest::WorkspaceOnly => Err(anyhow::anyhow!(
                "no [package] section — workspace-only manifests are dev \
                 environments, not releasable packages"
            )),
        }
    }

    pub fn name(&self) -> &PackageName {
        &self.package.name
    }

    pub fn version(&self) -> &Version {
        &self.package.version
    }

    /// Name and version together.
    pub fn identity(&self) -> PackageIdentity {
        PackageIdentity {
            name: self.package.name.clone(),
            version: self.package.version.clone(),
        }
    }

    /// Every dependency entry, across all of [`DEP_TABLES`].
    pub fn deps(&self) -> &[Dep] {
        &self.deps
    }

    pub fn build_number(&self) -> u64 {
        self.package
            .build
            .as_ref()
            .and_then(|b| b.config.as_ref())
            .map(|c| c.build_number)
            .unwrap_or(0)
    }

    /// `true` if this package builds a platform-independent (noarch) artifact:
    /// the `pixi-build-python` backend, or an `ament_python` ROS package. Those
    /// produce byte-identical output on every arch, so the buildfarm only builds
    /// them on linux-64. Anything else (ament_cmake, ament_idl, cmake, unknown)
    /// compiles per-arch and is not treated as noarch.
    pub fn is_noarch(&self) -> bool {
        let Some(build) = &self.package.build else {
            return false;
        };
        if build
            .backend
            .as_ref()
            .is_some_and(|b| b.name == "pixi-build-python")
        {
            return true;
        }
        build
            .config
            .as_ref()
            .is_some_and(|c| c.build_type == "ament_python")
    }

    /// `true` if the workspace's `platforms` list is empty or contains `target`.
    /// Empty list is treated as "no explicit restriction" (build everywhere).
    pub fn supports_platform(&self, target: Arch) -> bool {
        let Some(ws) = &self.workspace else {
            return true;
        };
        if ws.platforms.is_empty() {
            return true;
        }
        let target_str = target.to_string();
        ws.platforms.iter().any(|p| p == &target_str)
    }

    /// Relative `path =` values of every dep, excluding the
    /// self-as-workspace-member idiom (`path = "."`).
    pub fn path_dep_rel_paths(&self) -> Vec<String> {
        self.deps
            .iter()
            .filter_map(Dep::path)
            .filter(|p| *p != ".")
            .map(str::to_string)
            .collect()
    }

    /// Dep (key, pinned-version) pairs whose value is an exact `==X.Y.Z` string.
    /// These are committed opt-out pins (a released consumer that was decoupled
    /// from its sibling), hand-written or legacy.
    pub fn exact_pins(&self) -> Vec<(PackageName, Version)> {
        self.deps
            .iter()
            .filter_map(|d| {
                d.value
                    .as_str()
                    .and_then(exact_pin_version)
                    .map(|v| (d.name.clone(), v))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Discovered package
// ---------------------------------------------------------------------------

/// A package discovered on disk: its manifest parsed once, with the directory
/// that owns it. Commands take these rather than paths, so nothing downstream
/// re-reads the manifest or re-derives the directory from the file path.
#[derive(Debug)]
pub struct Package {
    /// Path to the package's `pixi.toml`, as given to discovery.
    pub manifest_path: PathBuf,
    /// The directory holding the manifest.
    pub dir: PathBuf,
    pub manifest: PackageManifest,
}

impl Package {
    /// Read a manifest that must declare a `[package]`; a workspace-only
    /// manifest is an error. Use [`Manifest::read`] where the workspace-only
    /// case is a legitimate outcome (discovery, routing probes).
    pub fn read(manifest_path: &Path) -> Result<Self> {
        match Manifest::read(manifest_path)? {
            Manifest::Package(manifest) => Ok(Self::new(manifest_path, manifest)),
            Manifest::WorkspaceOnly => anyhow::bail!(
                "{} has no [package] section — workspace-only manifests are dev \
                 environments, not releasable packages",
                manifest_path.display()
            ),
        }
    }

    fn new(manifest_path: &Path, manifest: PackageManifest) -> Self {
        Self {
            manifest_path: manifest_path.to_path_buf(),
            // A manifest path always has a parent; `pixi.toml` on its own has
            // the empty path, which joins and displays as the cwd-relative dir.
            dir: manifest_path
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf(),
            manifest,
        }
    }

    /// Name and version.
    pub fn identity(&self) -> PackageIdentity {
        self.manifest.identity()
    }
}

/// What discovery found at one candidate `pixi.toml`.
///
/// Outcomes rather than `Result<Option<Package>>` because the failures must not
/// all be `?`-able: discovery sweeps a directory nobody curated for it, and a
/// manifest it cannot use — missing `[package]`, TOML syntax error, or a key
/// that is missing or whose type does not match the schema — usually says
/// nothing about its neighbours. Returning the failure as a variant forces the sweep to decide
/// what to do with each kind instead of aborting on all of them.
enum Candidate {
    Package(Box<Package>),
    /// Parsed, but declares no `[package]` — see [`Manifest::WorkspaceOnly`].
    WorkspaceOnly,
    /// Unreadable or unparseable, in a way that says nothing about whether this
    /// was ever meant to be a releasable package.
    Unusable {
        path: PathBuf,
        error: anyhow::Error,
    },
    /// Declares a `[package]` that does not give this tool a readable `name`
    /// and `version` — either key absent, or spelled in a way our newtypes
    /// reject. This *is* a package, and it is one we would otherwise drop on
    /// the floor, so it is fatal even in a tolerant sweep.
    UnreadableIdentity {
        path: PathBuf,
        error: anyhow::Error,
    },
}

impl Candidate {
    fn read(manifest_path: &Path) -> Self {
        let text = match std::fs::read_to_string(manifest_path) {
            Ok(text) => text,
            Err(e) => {
                return Self::Unusable {
                    path: manifest_path.to_path_buf(),
                    error: anyhow::Error::new(e)
                        .context(format!("reading {}", manifest_path.display())),
                };
            }
        };
        match Manifest::parse(&text).with_context(|| format!("parsing {}", manifest_path.display()))
        {
            Ok(Manifest::Package(manifest)) => {
                Self::Package(Box::new(Package::new(manifest_path, manifest)))
            }
            Ok(Manifest::WorkspaceOnly) => Self::WorkspaceOnly,
            Err(error) => {
                let path = manifest_path.to_path_buf();
                if declares_unreadable_identity(&text) {
                    Self::UnreadableIdentity { path, error }
                } else {
                    Self::Unusable { path, error }
                }
            }
        }
    }

    /// The package, if this candidate is one.
    ///
    /// An unusable manifest is named in a warning and dropped — silence would
    /// make a typo in one manifest look like a package that simply isn't
    /// there. A manifest whose *identity* is unreadable is an error instead:
    /// dropping it would take a package that exists, and that a release run is
    /// very likely meant to include, out of the run without failing it.
    fn into_package(self) -> Result<Option<Package>> {
        match self {
            Self::Package(pkg) => Ok(Some(*pkg)),
            Self::WorkspaceOnly => Ok(None),
            Self::Unusable { path, error } => {
                tracing::warn!("skipping {}: {error:#}", path.display());
                Ok(None)
            }
            Self::UnreadableIdentity { path, error } => Err(error.context(format!(
                "{} declares a package this tool cannot identify; it would have been \
                 skipped and never built",
                path.display()
            ))),
        }
    }
}

/// Whether `text` is valid TOML declaring a `[package]` table whose `name` or
/// `version` is missing, or is not a string our own newtypes accept.
///
/// Deliberately structural rather than a match on the serde error text, and
/// deliberately limited to identity: a `[package]` with a bad `build-number` is
/// not fatal, because nothing about releasing *this* package reads it.
///
/// The table check is load-bearing. A `package` key that is not a table at all
/// (`package = "oops"`, a stray `[[package]]`) is a malformed manifest, not a
/// package that names itself unreadably — it stays a tolerated skip so one
/// broken file cannot abort discovery for its siblings.
fn declares_unreadable_identity(text: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return false;
    };
    let Some(package) = value.get("package").filter(|v| v.is_table()) else {
        return false;
    };
    let unreadable = |key: &str, accepts: fn(&str) -> bool| {
        !package
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(accepts)
    };
    unreadable("name", |s| PackageName::new(s).is_ok())
        || unreadable("version", |s| Version::parse(s).is_ok())
}

/// Discover per-package pixi workspaces under `package_dir`, parsing each
/// manifest exactly once.
///
/// If `filter` is `Some(name)`, returns only that package, and any problem
/// reading it is an error: an explicit request is answered or refused, never
/// quietly turned into an empty result.
///
/// Unfiltered, this is a tolerant sweep. Manifests with no `[package]` table
/// are skipped silently (see [`Manifest::WorkspaceOnly`]); manifests that fail
/// to read or parse are logged and skipped. Failure must not be contagious —
/// one mistyped key in a monorepo would otherwise block the release of every
/// unrelated package sitting beside it.
pub fn discover(package_dir: &Path, filter: Option<&PackageName>) -> Result<Vec<Package>> {
    // Root-package layout: package_dir itself holds the package's pixi.toml
    // (e.g. mise — a single-package repo with pixi.toml at its root). Only take
    // this branch when the root pixi.toml is a readable package; a
    // workspace-only or unusable root manifest is skipped and falls through to
    // the per-subdir scan so it doesn't shadow real packages under package_dir.
    // A root manifest with an unreadable *identity* is the exception: fatal,
    // because silently skipping it would drop a real package from the release.
    let root_pixi = package_dir.join(PIXI_TOML);
    if root_pixi.exists()
        && let Some(pkg) = Candidate::read(&root_pixi).into_package()?
    {
        let id = pkg.identity();
        match filter {
            Some(name) if *name != id.name => {
                anyhow::bail!("package {name} not found; root package is {}", id.name)
            }
            _ => return Ok(vec![pkg]),
        }
    }

    if let Some(name) = filter {
        let pixi = package_dir.join(name.as_str()).join(PIXI_TOML);
        if !pixi.exists() {
            anyhow::bail!("package {name} not found at {}", pixi.display());
        }
        // An explicit request naming something unreleasable says so, rather
        // than letting the caller trip over a missing [package] later.
        let manifest = match Manifest::read(&pixi)? {
            Manifest::Package(m) => m,
            Manifest::WorkspaceOnly => anyhow::bail!(
                "package {name} has no [package] section in {} — workspace-only \
                 manifests are dev environments, not releasable packages",
                pixi.display()
            ),
        };
        return Ok(vec![Package::new(&pixi, manifest)]);
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(package_dir)
        .with_context(|| format!("reading {}", package_dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let pixi = entry.path().join(PIXI_TOML);
        if !pixi.exists() {
            continue;
        }
        if let Some(pkg) = Candidate::read(&pixi).into_package()? {
            out.push(pkg);
        }
    }
    out.sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
    Ok(out)
}

/// A pixi manifest `mise ci test` can run pixi commands against — either a
/// releasable [`Package`] or a workspace-only manifest with no `[package]`
/// table (e.g. a docker-build variant workspace that has no publishable
/// package but still has pixi environments/tasks to install and test).
/// Running tests never reads package identity, only the manifest path and
/// the directory pixi commands run in.
#[derive(Debug)]
pub struct TestTarget {
    pub manifest_path: PathBuf,
    pub dir: PathBuf,
}

impl TestTarget {
    fn new(manifest_path: &Path) -> Self {
        Self {
            manifest_path: manifest_path.to_path_buf(),
            dir: manifest_path
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf(),
        }
    }
}

/// Discover pixi manifests to test under `package_dir` — like [`discover`],
/// but also includes workspace-only manifests alongside releasable packages,
/// since `mise ci test` runs `pixi install`/`pixi run` against any pixi
/// manifest and has no use for package identity. Same root-package-layout
/// fallback and filter semantics as [`discover`] otherwise.
pub fn discover_test_targets(
    package_dir: &Path,
    filter: Option<&PackageName>,
) -> Result<Vec<TestTarget>> {
    let root_pixi = package_dir.join(PIXI_TOML);

    if let Some(name) = filter {
        let pixi = package_dir.join(name.as_str()).join(PIXI_TOML);
        if pixi.exists() {
            Manifest::read(&pixi)?; // fail loudly here, not inside pixi
            return Ok(vec![TestTarget::new(&pixi)]);
        }
        // Root-package layout fallback: package_dir itself is the manifest
        // (e.g. a single-package repo run with `--package-dir .`).
        if root_pixi.exists()
            && let Manifest::Package(m) = Manifest::read(&root_pixi)?
            && m.identity().name == *name
        {
            return Ok(vec![TestTarget::new(&root_pixi)]);
        }
        anyhow::bail!("package {name} not found at {}", pixi.display());
    }

    if root_pixi.exists() && Manifest::read(&root_pixi).is_ok() {
        return Ok(vec![TestTarget::new(&root_pixi)]);
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(package_dir)
        .with_context(|| format!("reading {}", package_dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let pixi = entry.path().join(PIXI_TOML);
        if !pixi.exists() {
            continue;
        }
        match Manifest::read(&pixi) {
            Ok(_) => out.push(TestTarget::new(&pixi)),
            Err(error) => tracing::warn!("skipping {}: {error:#}", pixi.display()),
        }
    }
    out.sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Edit view (format-preserving)
// ---------------------------------------------------------------------------

/// Set `[package].version`, preserving every comment, key order and blank line
/// in the rest of the document.
///
/// Text in, text out, and not pixi-specific: Cargo.toml has the same
/// `[package] version` shape and `ci sync-cargo` bumps it through here too.
/// Errors if there is no `[package]` table or it has no `version` key —
/// creating either would be papering over a manifest that isn't a package.
pub fn set_package_version(toml_text: &str, version: &Version) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = toml_text.parse().context("parsing TOML")?;
    let package = doc
        .get_mut("package")
        .ok_or_else(|| anyhow::anyhow!("no [package] table found"))?
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[package] is not a table"))?;
    if !package.contains_key("version") {
        anyhow::bail!("no version key in [package] table");
    }
    package["version"] = toml_edit::value(version.to_string());
    Ok(doc.to_string())
}

/// Rewrite `[package.build.config].build-number` in `manifest_path` to `value`,
/// creating the intermediate tables if absent.
pub fn set_build_number(manifest_path: &Path, value: u64) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {} as TOML", manifest_path.display()))?;

    let package = doc
        .get_mut("package")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| anyhow::anyhow!("{}: missing [package] table", manifest_path.display()))?;

    if !package.contains_key("build") {
        package.insert("build", toml_edit::table());
    }
    let build = package
        .get_mut("build")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{}: [package.build] exists but is not a table",
                manifest_path.display(),
            )
        })?;

    if !build.contains_key("config") {
        build.insert("config", toml_edit::table());
    }
    let config = build
        .get_mut("config")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{}: [package.build.config] exists but is not a table",
                manifest_path.display(),
            )
        })?;

    config.insert(
        "build-number",
        toml_edit::value(i64::try_from(value).context("build-number exceeds i64")?),
    );

    std::fs::write(manifest_path, doc.to_string())
        .with_context(|| format!("write {}", manifest_path.display()))?;
    Ok(())
}

/// Front-insert channels into `[workspace].channels`, so local just-built
/// artifacts win over the real channel during the solve.
pub fn prepend_channels(manifest_path: &Path, channels: &[ChannelUrl]) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)?;
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let arr = doc["workspace"]["channels"].as_array_mut().ok_or_else(|| {
        anyhow::anyhow!("{}: no workspace.channels array", manifest_path.display())
    })?;
    for (i, ch) in channels.iter().enumerate() {
        arr.insert(i, ch.to_string());
    }
    std::fs::write(manifest_path, doc.to_string())?;
    Ok(())
}

/// A sibling package whose `path =` dep was rewritten to a derived
/// `>=version,<major+1` pin in the temp checkout. `version` is the exact
/// floor — availability checks and fallback builds key on it, never on
/// "anything in range", so a coupled release always builds against the
/// fresh sibling.
#[derive(Debug)]
pub struct ResolvedDep {
    /// The dependency *key* in the consumer's manifest (e.g. `ros-kilted-lib`).
    /// This is the channel artifact name, which is what `ChannelIndex` /
    /// `version_published` / `built_this_job` / the local-build guard key off
    /// of. It is NOT necessarily the sibling's `package.name`: the published
    /// artifact name is prefixed/transformed relative to it. The dep key is
    /// guaranteed correct because the consumer's manifest can only solve
    /// against the real channel name.
    pub name: PackageName,
    pub version: Version,
    /// The sibling's pixi.toml inside the same checkout (used for fallback local builds).
    pub manifest: PathBuf,
}

/// Rewrite every non-self `path =` dep in the manifest to a version pin in
/// `style`, reading the version from the sibling manifest at the same rev. The
/// derived pin is deterministic: same rev -> same sibling manifest -> same
/// pin. The default [`SiblingPinStyle::Range`] lets already-published
/// consumers accept future sibling releases within the major without a
/// re-release; [`SiblingPinStyle::Exact`] is the lockstep opt-in.
///
/// This is only ever called by the farm (via `mise build-recipes`) on
/// ephemeral temp checkouts; the committed manifest keeps its path deps.
pub fn resolve_path_deps(manifest_path: &Path, style: SiblingPinStyle) -> Result<Vec<ResolvedDep>> {
    let manifest_dir = manifest_path.parent().unwrap_or(Path::new(""));
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", manifest_path.display()))?;

    let mut resolved = Vec::new();
    for_each_dep_table_mut(&mut doc, &mut |table| {
        rewrite_path_deps_in(table, manifest_dir, style, &mut resolved)
    })?;

    std::fs::write(manifest_path, doc.to_string())?;
    Ok(resolved)
}

/// Apply `f` to each dependency table present in `doc`, in [`DEP_TABLES`] order.
fn for_each_dep_table_mut(
    doc: &mut toml_edit::DocumentMut,
    f: &mut dyn FnMut(&mut dyn toml_edit::TableLike) -> Result<()>,
) -> Result<()> {
    for table_path in DEP_TABLES {
        if let Some(table) = table_at_mut(doc.as_item_mut(), table_path) {
            f(table)?;
        }
    }
    Ok(())
}

/// Walk `path` from `item` to the table it names. Recursive rather than a loop
/// because reassigning a `&mut` borrow inside a loop does not pass the borrow
/// checker.
fn table_at_mut<'a>(
    item: &'a mut toml_edit::Item,
    path: &[&str],
) -> Option<&'a mut dyn toml_edit::TableLike> {
    match path.split_first() {
        None => item.as_table_like_mut(),
        Some((seg, rest)) => table_at_mut(item.get_mut(seg)?, rest),
    }
}

/// Rewrite the sibling `path =` deps of one table to a `style` version pin.
/// A free fn rather than a closure because the closure form fights the borrow
/// checker across the `get_mut` walk.
fn rewrite_path_deps_in(
    table: &mut dyn toml_edit::TableLike,
    manifest_dir: &Path,
    style: SiblingPinStyle,
    resolved: &mut Vec<ResolvedDep>,
) -> Result<()> {
    let keys: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
    for key in keys {
        let Some(item) = table.get(&key) else {
            continue;
        };
        let Some(path) = item
            .as_table_like()
            .and_then(|t| t.get("path"))
            .and_then(|p| p.as_str())
        else {
            continue;
        };
        if path == "." {
            continue; // self-as-workspace-member idiom
        }
        let sib_manifest = manifest_dir.join(path).join(PIXI_TOML);
        let sib_text = std::fs::read_to_string(&sib_manifest).with_context(|| {
            format!(
                "path dep {key}: no pixi.toml at {} in checkout",
                sib_manifest.display()
            )
        })?;
        // Only `package.version` is read from the sibling manifest: the
        // dependency key, not the sibling's `package.name`, is the channel
        // artifact name.
        let version = PackageManifest::parse(&sib_text)
            .with_context(|| {
                format!(
                    "path dep {key}: parsing sibling manifest {}",
                    sib_manifest.display()
                )
            })?
            .version()
            .clone();
        table.insert(&key, toml_edit::value(style.pin(&version)));
        resolved.push(ResolvedDep {
            name: PackageName::new(key)?,
            version,
            manifest: sib_manifest,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
