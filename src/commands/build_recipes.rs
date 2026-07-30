use std::path::PathBuf;

use clap::Subcommand;

use crate::types::{Arch, DeepstreamVersion, RecipeName, RunnerSize, TargetPlatform};

#[derive(Subcommand, Debug)]
pub enum BuildRecipes {
    /// Build the vinca pipeline.
    Vinca {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: String,
        /// Extra channel whose already-published packages should be skipped
        /// (rattler `--skip-existing`) but which must NOT win dependency
        /// resolution — the `overrides` channel. Its packages carry
        /// `down_prioritize_variant`, so the solver avoids them for build deps
        /// while `--skip-existing` still finds them to skip a rebuild.
        #[arg(long)]
        overrides_channel_url: Option<String>,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: TargetPlatform,
        #[arg(long = "ds-recipe")]
        ds_recipes: Vec<RecipeName>,
        #[arg(long)]
        ds_version: Option<DeepstreamVersion>,
        /// Build only the listed recipe(s) — for local debugging. Mutually
        /// exclusive with --ds-recipe. Combine with --ds-version to pin the
        /// DS axis when debugging a DeepStream recipe.
        #[arg(long = "only")]
        only: Vec<RecipeName>,
    },
    /// Build pixi-native packages.
    Pixi {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: String,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: TargetPlatform,
        /// Optional filter: only build entries with this runner-size.
        #[arg(long)]
        runner_size: Option<RunnerSize>,
        /// Build only the listed package(s) by name. Empty = build all.
        #[arg(long = "only")]
        only: Vec<String>,
    },
    /// Run a vinca build inside a DeepStream container. Does container-side prep
    /// (git auth, cache cleanup, `pixi install`) and delegates to `build vinca`
    /// with `--ds-version` and the recipe list pinned.
    DeepstreamContainer {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: String,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: TargetPlatform,
        #[arg(long = "ds-recipe", required = true)]
        ds_recipes: Vec<RecipeName>,
        #[arg(long)]
        ds_version: DeepstreamVersion,
    },
}

impl BuildRecipes {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Vinca {
                repo_root,
                channel_url,
                overrides_channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
                only,
            } => vinca(
                repo_root,
                channel_url,
                overrides_channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
                only,
            ),
            Self::Pixi {
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                runner_size,
                only,
            } => pixi(
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                runner_size,
                &only,
            ),
            Self::DeepstreamContainer {
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
            } => deepstream_container(
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
            ),
        }
    }
}

use crate::consts::{PIXI_TOML, ROBOSTACK_CHANNEL};
use crate::gh;
use crate::git;
use crate::process;
use crate::repo::Repo;

#[allow(clippy::too_many_arguments)]
fn vinca(
    repo_root: Option<PathBuf>,
    channel_url: String,
    overrides_channel_url: Option<String>,
    output_dir: PathBuf,
    target_platform: TargetPlatform,
    ds_recipes: Vec<RecipeName>,
    ds_version: Option<DeepstreamVersion>,
    only: Vec<RecipeName>,
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let mode = VincaBuildMode::from_flags(ds_recipes, ds_version, only)?;

    // rattler-build fetches recipe sources over git, including from private
    // repos; mise owns that auth setup (see gh::ensure_git_auth).
    gh::ensure_git_auth()?;

    let abs_output = if output_dir.is_absolute() {
        output_dir
    } else {
        repo.root().join(&output_dir)
    };
    fs::create_dir_all(&abs_output).with_context(|| format!("mkdir {}", abs_output.display()))?;

    let arch_str = target_platform.arch().to_string();

    // 1. Generate `./recipes/` from vinca.yaml.
    process::run_in(
        repo.root(),
        "pixi",
        &["run", "vinca", "-m", "--platform", &arch_str],
    )?;

    // 2. Manipulate recipes/ per mode.
    apply_recipe_filter(repo.root(), &mode)?;

    // 3. Prepare variants args (and hold any temp file alive for the duration).
    let mut variant_args: Vec<String> = vec!["-m".into(), "conda_build_config.yaml".into()];
    let _pin = if let Some(v) = mode.version() {
        let tf = write_variants_pin(v)?;
        variant_args.push("-m".into());
        variant_args.push(tf.path().to_string_lossy().into_owned());
        Some(tf)
    } else {
        variant_args.push("-m".into());
        variant_args.push("variants/deepstream.yaml".into());
        None
    };

    // 4. Build.
    let abs_output_str = abs_output.to_string_lossy().into_owned();
    let mut args: Vec<&str> = vec![
        "run",
        "rattler-build",
        "build",
        "--recipe-dir",
        "./recipes",
        "--target-platform",
        &arch_str,
    ];
    for a in &variant_args {
        args.push(a);
    }
    args.extend_from_slice(&["-c", &channel_url]);
    // The overrides channel lets --skip-existing find already-published
    // override packages (so they aren't rebuilt every run). It sits below the
    // general channel; its packages carry down_prioritize_variant so the solver
    // still prefers the stock build for any dependency.
    if let Some(ovr) = &overrides_channel_url {
        args.push("-c");
        args.push(ovr.as_str());
    }
    args.extend_from_slice(&[
        "-c",
        ROBOSTACK_CHANNEL,
        "-c",
        "https://prefix.dev/conda-forge",
        "--skip-existing=all",
        "--output-dir",
        &abs_output_str,
    ]);
    process::run_in(repo.root(), "pixi", &args)?;

    Ok(())
}

use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Selects which subset of recipes to build and whether to pin a DeepStream version.
/// Maps to the valid combinations of `--ds-recipe`, `--ds-version`, and `--only` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VincaBuildMode {
    /// No flags: build everything in `recipes/` across all DS variants.
    Normal,
    /// `--ds-recipe NAME [...]` without `--ds-version`: drop the listed DS recipes,
    /// build everything else across all DS variants.
    DropDeepstream { recipes: Vec<RecipeName> },
    /// `--ds-recipe NAME [...]` plus `--ds-version V`: keep only the listed DS recipes,
    /// pin the DS axis to the given version.
    DeepstreamOnly {
        recipes: Vec<RecipeName>,
        version: DeepstreamVersion,
    },
    /// `--only NAME [...]` (with or without `--ds-version`): keep only the listed
    /// recipes regardless of DS-ness. For local debugging. When `version` is set,
    /// pin the DS axis (useful when the listed recipe is a DS one).
    Only {
        recipes: Vec<RecipeName>,
        version: Option<DeepstreamVersion>,
    },
}

impl VincaBuildMode {
    /// Construct from the parsed CLI flags. Rejects `--ds-version` without either
    /// `--ds-recipe` or `--only` (would build everything against one pinned DS
    /// version — almost certainly a misconfiguration). Rejects `--only` combined
    /// with `--ds-recipe` (ambiguous; the two filters mean different things).
    pub fn from_flags(
        recipes: Vec<RecipeName>,
        version: Option<DeepstreamVersion>,
        only: Vec<RecipeName>,
    ) -> anyhow::Result<Self> {
        if !only.is_empty() && !recipes.is_empty() {
            anyhow::bail!("--only and --ds-recipe are mutually exclusive");
        }
        if !only.is_empty() {
            return Ok(Self::Only {
                recipes: only,
                version,
            });
        }
        match (recipes.is_empty(), version) {
            (true, None) => Ok(Self::Normal),
            (false, None) => Ok(Self::DropDeepstream { recipes }),
            (false, Some(version)) => Ok(Self::DeepstreamOnly { recipes, version }),
            (true, Some(_)) => {
                anyhow::bail!("--ds-version requires at least one --ds-recipe or --only recipe")
            }
        }
    }

    /// The DeepStream version this mode pins the variant axis to, if any.
    pub fn version(&self) -> Option<DeepstreamVersion> {
        match self {
            Self::DeepstreamOnly { version, .. } => Some(*version),
            Self::Only { version, .. } => *version,
            _ => None,
        }
    }
}

/// Manipulate the `<repo>/recipes` directory before invoking rattler-build:
///
/// 1. Overlay each entry from `<repo>/vendor_recipes/` onto `recipes/`, overwriting
///    existing dirs (vendor recipes win — they're handrolled and the vinca-generated
///    versions in `recipes/` are stale).
/// 2. Remove `recipes/deepstream-mutex` unconditionally (a payload-less noarch
///    metapackage published by `bootstrap-mutex.yml`; consumed from the channel,
///    never built here).
/// 3. Apply the mode-specific filter:
///    - `Normal` → no further filtering.
///    - `DropDeepstream { recipes }` → remove each listed recipe dir.
///    - `DeepstreamOnly { recipes, .. }` → remove every recipe dir whose name is NOT
///      in the listed set.
///    - `Only { recipes, .. }` → same keep-only sweep as `DeepstreamOnly`, but
///      independent of DS-ness (used for local single-recipe debugging).
fn apply_recipe_filter(repo_root: &Path, mode: &VincaBuildMode) -> anyhow::Result<()> {
    let recipes_dir = repo_root.join("recipes");
    let vendor_dir = repo_root.join("vendor_recipes");

    if vendor_dir.is_dir() {
        for entry in
            fs::read_dir(&vendor_dir).with_context(|| format!("read {}", vendor_dir.display()))?
        {
            let entry = entry?;
            let src = entry.path();
            let name = entry.file_name();
            let dst = recipes_dir.join(&name);
            if dst.exists() {
                fs::remove_dir_all(&dst).with_context(|| format!("remove {}", dst.display()))?;
            }
            copy_dir_all(&src, &dst)
                .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
        }
    }

    // Always drop the mutex metapackage if vinca emitted one.
    let mutex = recipes_dir.join("deepstream-mutex");
    if mutex.exists() {
        fs::remove_dir_all(&mutex).with_context(|| format!("remove {}", mutex.display()))?;
    }

    match mode {
        VincaBuildMode::Normal => {}
        VincaBuildMode::DropDeepstream { recipes } => {
            for r in recipes {
                let p = recipes_dir.join(r.as_str());
                if p.exists() {
                    fs::remove_dir_all(&p).with_context(|| format!("remove {}", p.display()))?;
                }
            }
        }
        VincaBuildMode::DeepstreamOnly { recipes, .. } | VincaBuildMode::Only { recipes, .. } => {
            let keep: std::collections::HashSet<&str> =
                recipes.iter().map(|r| r.as_str()).collect();
            for entry in fs::read_dir(&recipes_dir)
                .with_context(|| format!("read {}", recipes_dir.display()))?
            {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !keep.contains(name_str.as_ref()) {
                    fs::remove_dir_all(entry.path())
                        .with_context(|| format!("remove {}", entry.path().display()))?;
                }
            }
        }
    }

    Ok(())
}

/// Recursively copy `src` to `dst`, creating `dst` if needed.
fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

use tempfile::NamedTempFile;

/// Write a one-off variants YAML pinning the DS axis (and, for DS 7.1, the gcc
/// version that nvcc accepts). Returned `NamedTempFile` lives as long as the
/// caller keeps it; rattler-build reads the path before it's dropped.
///
/// `None` means no pin — the caller should pass `variants/deepstream.yaml`
/// (the full variants file with both DS versions) to rattler-build instead.
fn write_variants_pin(version: DeepstreamVersion) -> anyhow::Result<NamedTempFile> {
    // rattler-build's `-m` flag takes file paths, not KEY=VALUE. Passing
    // `variants/deepstream.yaml` would expand over every listed version.
    // DS 7.1's CUDA 12.6 nvcc rejects host gcc > 13 as -ccbin; DS 8.0 (CUDA
    // 12.8) accepts gcc 14 — so 7.1 needs an explicit gcc pin alongside.
    let mut content = format!("deepstream_version:\n  - \"{version}\"\n");
    if version == DeepstreamVersion::V7_1 {
        content.push_str("c_compiler_version:\n  - \"13\"\n");
        content.push_str("cxx_compiler_version:\n  - \"13\"\n");
    }
    let mut tf = tempfile::Builder::new()
        .prefix("ds-pin.")
        .suffix(".yaml")
        .tempfile()
        .context("create temp variants file")?;
    use std::io::Write;
    tf.write_all(content.as_bytes())
        .context("write temp variants file")?;
    tf.flush().context("flush temp variants file")?;
    Ok(tf)
}

use serde::Deserialize;

/// Subset of `pixi.toml` consumed by build pixi.
/// Only the fields we read are listed; serde ignores the rest.
#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamPixiToml {
    pub package: UpstreamPackage,
    #[serde(default)]
    pub workspace: Option<UpstreamWorkspace>,
    #[serde(default)]
    dependencies: Option<toml::value::Table>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamPackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub build: Option<UpstreamBuild>,
    #[serde(default, rename = "run-dependencies")]
    run_dependencies: Option<toml::value::Table>,
    #[serde(default, rename = "host-dependencies")]
    host_dependencies: Option<toml::value::Table>,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: Option<toml::value::Table>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamBuild {
    #[serde(default)]
    pub backend: Option<UpstreamBuildBackend>,
    #[serde(default)]
    pub config: Option<UpstreamBuildConfig>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamBuildBackend {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamBuildConfig {
    /// Defaults to 0 when omitted from the upstream pixi.toml.
    #[serde(default, rename = "build-number")]
    pub build_number: u64,
    #[serde(default, rename = "build-type")]
    pub build_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamWorkspace {
    #[serde(default)]
    pub platforms: Vec<String>,
}

impl UpstreamPixiToml {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        toml::from_str(text).context("parse upstream pixi.toml")
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
    /// them on linux-64 (see the skip in `check_entry`). Anything else
    /// (ament_cmake, ament_idl, cmake, unknown) compiles per-arch and is not
    /// treated as noarch.
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
    pub fn supports_platform(&self, target: TargetPlatform) -> bool {
        let Some(ws) = &self.workspace else {
            return true;
        };
        if ws.platforms.is_empty() {
            return true;
        }
        let target_str = target.arch().to_string();
        ws.platforms.iter().any(|p| p == &target_str)
    }

    /// Relative `path =` values from all dependency tables, excluding the
    /// self-as-workspace-member idiom (`path = "."`).
    pub fn path_dep_rel_paths(&self) -> Vec<String> {
        let tables = [
            self.dependencies.as_ref(),
            self.package.run_dependencies.as_ref(),
            self.package.host_dependencies.as_ref(),
            self.package.build_dependencies.as_ref(),
        ];
        let mut out = Vec::new();
        for table in tables.into_iter().flatten() {
            for value in table.values() {
                if let Some(p) = value.get("path").and_then(|p| p.as_str())
                    && p != "."
                {
                    out.push(p.to_string());
                }
            }
        }
        out
    }

    /// Dep (key, pinned-version) pairs whose value is an exact `==X.Y.Z` string
    /// across all dep tables. These are committed opt-out pins (a released
    /// consumer that was decoupled from its sibling), hand-written or legacy.
    fn exact_pins(&self) -> Vec<(String, String)> {
        let tables = [
            self.dependencies.as_ref(),
            self.package.run_dependencies.as_ref(),
            self.package.host_dependencies.as_ref(),
            self.package.build_dependencies.as_ref(),
        ];
        let mut out = Vec::new();
        for table in tables.into_iter().flatten() {
            for (key, value) in table {
                if let Some(v) = value.as_str().and_then(exact_pin_version) {
                    out.push((key.clone(), v.to_string()));
                }
            }
        }
        out
    }
}

/// If `s` is an exact `==X.Y.Z` pin, return the version (`X.Y.Z[-pre]`); else
/// `None`. Exact means `==` + a single concrete version with three numeric
/// core components. Ranges (`>=`, `<`), wildcards (`==1.2.*`), and build specs
/// are not pins.
fn exact_pin_version(s: &str) -> Option<&str> {
    let v = s.trim().strip_prefix("==")?;
    let core = v.split('-').next().unwrap_or(v);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        Some(v)
    } else {
        None
    }
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
    /// of. It is NOT
    /// necessarily the sibling's `package.name` — in package-xml mode the
    /// sibling manifest may have no `package.name` at all, and even when it
    /// does, the published artifact is prefixed/transformed relative to it.
    /// The dep key is guaranteed correct because the consumer's manifest can
    /// only solve against the real channel name.
    pub name: String,
    pub version: String,
    /// The sibling's pixi.toml inside the same checkout (used for fallback local builds).
    pub manifest: PathBuf,
}

/// Derived pin for a sibling at `version`: `>=<version>,<<major+1>`. The floor
/// is the sibling's version at the consumer's tagged rev (so a side-by-side
/// release ratchets it to the fresh version); the major cap is the same trust
/// model as committed cross-repo internal pins. Prerelease floors are fine:
/// `>=1.24.0-alpha.2,<2` admits the prerelease and everything after it.
fn range_pin(version: &str) -> anyhow::Result<String> {
    let major: u64 = version
        .split(['.', '-'])
        .next()
        .unwrap_or("")
        .parse()
        .with_context(|| format!("version {version} does not start with a numeric major"))?;
    Ok(format!(">={version},<{}", major + 1))
}

/// Rewrite `path =` deps in a single table-like section (e.g. `[dependencies]`
/// or `[package.run-dependencies]`) to `">=<version>,<major+1>"`, skipping the
/// self-as-workspace-member idiom (`path = "."`). Lifted out of
/// `resolve_path_deps` as a free fn because the closure form fights the
/// borrow checker across the `doc.get_mut` calls.
fn visit_table(
    table: &mut dyn toml_edit::TableLike,
    manifest_dir: &Path,
    resolved: &mut Vec<ResolvedDep>,
) -> anyhow::Result<()> {
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
            continue;
        }
        let sib_manifest = manifest_dir.join(path).join(PIXI_TOML);
        let sib_text = fs::read_to_string(&sib_manifest).with_context(|| {
            format!(
                "path dep {key}: no pixi.toml at {} in checkout",
                sib_manifest.display()
            )
        })?;
        // Only `package.version` is read from the sibling manifest: the
        // dependency key (not the sibling's `package.name`, which may not
        // even exist in package-xml mode) is the channel artifact name.
        let sib_doc: toml::Value = toml::from_str(&sib_text)
            .with_context(|| format!("parse sibling manifest for {key}"))?;
        let version = sib_doc
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
            .with_context(|| {
                format!(
                    "path dep {key}: sibling manifest {} has no package.version",
                    sib_manifest.display()
                )
            })?
            .to_string();
        table.insert(&key, toml_edit::value(range_pin(&version)?));
        resolved.push(ResolvedDep {
            name: key.clone(),
            version,
            manifest: sib_manifest,
        });
    }
    Ok(())
}

/// Rewrite every non-self `path =` dep in the manifest to
/// `">=<version>,<major+1>"`, reading the version from the sibling manifest at
/// the same rev. The derived pin is deterministic: same rev -> same sibling
/// manifest -> same pin. The range (rather than `==`) lets already-published
/// consumers accept future sibling releases within the major without a
/// re-release.
///
/// This is only ever called by the farm (via `mise build-recipes`) on
/// ephemeral temp checkouts; the committed manifest keeps its path deps.
pub(crate) fn resolve_path_deps(manifest_path: &Path) -> anyhow::Result<Vec<ResolvedDep>> {
    let manifest_dir = manifest_path.parent().unwrap();
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", manifest_path.display()))?;

    let mut resolved = Vec::new();

    if let Some(table) = doc
        .get_mut("dependencies")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        visit_table(table, manifest_dir, &mut resolved)?;
    }
    if let Some(pkg) = doc
        .get_mut("package")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        for section in [
            "run-dependencies",
            "host-dependencies",
            "build-dependencies",
        ] {
            if let Some(table) = pkg
                .get_mut(section)
                .and_then(toml_edit::Item::as_table_like_mut)
            {
                visit_table(table, manifest_dir, &mut resolved)?;
            }
        }
    }

    fs::write(manifest_path, doc.to_string())?;
    Ok(resolved)
}

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
fn resolve_sibling_pins(
    manifest_path: &Path,
    workdir: &Path,
    sibling_subdirs: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<Vec<ResolvedDep>> {
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let upstream = UpstreamPixiToml::parse(&text)
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
        let sib_version = UpstreamPixiToml::parse(&sib_text)
            .with_context(|| format!("parse sibling manifest for {name}"))?
            .package
            .version;
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
fn sibling_subdirs(entry: &PixiNativeEntry, all: &[PixiNativeEntry]) -> BTreeMap<String, PathBuf> {
    all.iter()
        .filter(|e| {
            e.url.owner == entry.url.owner && e.url.repo == entry.url.repo && e.name != entry.name
        })
        .map(|e| {
            (
                e.name.clone(),
                e.subdir.clone().unwrap_or_else(|| PathBuf::from(".")),
            )
        })
        .collect()
}

/// Front-insert channels into [workspace].channels of the temp manifest, so
/// local just-built artifacts win over the real channel during the solve.
fn prepend_channels(manifest_path: &Path, channels: &[String]) -> anyhow::Result<()> {
    let text = fs::read_to_string(manifest_path)?;
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let arr = doc["workspace"]["channels"].as_array_mut().ok_or_else(|| {
        anyhow::anyhow!("{}: no workspace.channels array", manifest_path.display())
    })?;
    for (i, ch) in channels.iter().enumerate() {
        arr.insert(i, ch.as_str());
    }
    fs::write(manifest_path, doc.to_string())?;
    Ok(())
}

/// Push `s` onto `v` unless it's already present.
fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x == &s) {
        v.push(s);
    }
}

/// Guard for `build_local_dep`'s recursion, factored out so it's testable
/// without invoking pixi. `Ok(false)` means skip (already built this entry),
/// `Ok(true)` means proceed, `Err` means a cycle was detected among the
/// sibling path deps being built as local fallbacks.
fn check_local_build_guard(
    name: &str,
    visiting: &[String],
    local_built: &BTreeSet<String>,
) -> anyhow::Result<bool> {
    if visiting.iter().any(|v| v == name) {
        anyhow::bail!(
            "path-dep cycle among local fallback builds: {} -> {}",
            visiting.join(" -> "),
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
struct LocalBuildCtx<'a> {
    local_deps_dir: &'a Path,
    /// Snapshot of the upstream channel, swept once for the whole job.
    channel: &'a ChannelIndex,
    target_platform: TargetPlatform,
    /// Repo checkout root, for resolving same-repo sibling pins.
    workdir: &'a Path,
    /// dep-name -> repo-relative subdir for same-repo siblings.
    sibling_subdirs: &'a BTreeMap<String, PathBuf>,
}

/// Build a path-dep sibling from the consumer's checkout into a local-only
/// file channel. Recurses through the sibling's own path deps and same-repo
/// pins first. `local_built` and `visiting` are scoped per top-level entry
/// (fresh at each call site in the main build loop): `local_built` dedupes a
/// diamond dependency shared by two siblings, and `visiting` catches a cycle
/// among path deps before it reaches pixi's solver.
/// ponytail: duplicate build when the sibling lives in another matrix job;
/// layered farm stages are the upgrade path if this gets slow in practice.
fn build_local_dep(
    dep: &ResolvedDep,
    ctx: &LocalBuildCtx<'_>,
    local_built: &mut BTreeSet<String>,
    visiting: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !check_local_build_guard(&dep.name, visiting, local_built)? {
        return Ok(());
    }
    fs::create_dir_all(ctx.local_deps_dir)?;
    visiting.push(dep.name.clone());
    // Same read-before-rewrite ordering as the main loop.
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
    if built_any_nested {
        prepend_channels(
            &dep.manifest,
            &[format!("file://{}", ctx.local_deps_dir.display())],
        )?;
    }
    let target_channel = format!("file://{}", ctx.local_deps_dir.display());
    let arch = ctx.target_platform.arch().to_string();
    process::run("pixi", &publish_argv(&dep.manifest, &target_channel, &arch))?;
    local_built.insert(dep.name.clone());
    Ok(())
}

/// A pixi-native entry selected for building, along with the info needed to
/// order it relative to other builds (see `topo_sort_builds`).
#[derive(Debug)]
pub(crate) struct BuildItem<'a> {
    pub entry: &'a PixiNativeEntry,
    pub effective_build: u64,
    pub name: String,
    pub rel_path_deps: Vec<String>,
    /// Dep keys of committed `==` pins (channel artifact names). A same-repo
    /// sibling matching one must build first (opt-out coupling still needs
    /// same-bucket ordering).
    pub pin_dep_names: Vec<String>,
}

/// Order build items so same-repo dependency targets build before consumers.
/// Path-dep edge: consumer.subdir/rel_path (normalized) == target.subdir, same
/// url. Pin edge: consumer's `==` pin key == target `entry.name`, same url.
fn topo_sort_builds(items: Vec<BuildItem<'_>>) -> anyhow::Result<Vec<BuildItem<'_>>> {
    use crate::commands::ci::siblings::normalize;

    let key = |e: &PixiNativeEntry| {
        (
            format!("{}/{}", e.url.owner, e.url.repo),
            normalize(e.subdir.as_deref().unwrap_or(Path::new("."))),
        )
    };
    let repo_of = |e: &PixiNativeEntry| format!("{}/{}", e.url.owner, e.url.repo);
    let index: BTreeMap<_, usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (key(it.entry), i))
        .collect();
    // (repo, entry.name) -> index, for pin edges keyed on the artifact name.
    let name_index: BTreeMap<(String, String), usize> = items
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
    // Reorder without cloning items.
    let mut slots: Vec<Option<BuildItem>> = items.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| slots[i].take().unwrap())
        .collect())
}

use crate::types::{GithubRepoUrl, PixiNativeEntry, Sha40};

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
        &entry.url.owner,
        &entry.url.repo,
        entry.rev.as_str(),
        &entry_manifest_rel_path(entry),
    )
    .with_context(|| format!("entry {}", entry.name))
}

/// Materialize one commit of an entry's repo in `dest`.
fn fetch_at_rev(url: &GithubRepoUrl, rev: &Sha40, dest: &Path) -> anyhow::Result<()> {
    git::fetch_rev(
        dest,
        &format!("https://github.com/{}/{}", url.owner, url.repo),
        rev,
    )
}

/// `pixi publish` argv. Built from `OsStr` so a non-UTF-8 manifest path is
/// passed through rather than panicking a `to_str().unwrap()`.
fn publish_argv<'a>(
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
    target_platform: TargetPlatform,
) -> anyhow::Result<Option<BTreeMap<String, Vec<SearchRecord>>>> {
    let arch = target_platform.arch().to_string();

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
enum BuildSubdir {
    Noarch,
    Arch(Arch),
}

impl BuildSubdir {
    /// Where `upstream` publishes to when built for `target_platform`.
    fn of(upstream: &UpstreamPixiToml, target_platform: TargetPlatform) -> Self {
        if upstream.is_noarch() {
            Self::Noarch
        } else {
            Self::Arch(target_platform.arch())
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
/// Do not point this at a public channel: a `'*'` glob makes the gateway pull
/// the channel's full name index and then fetch records per match, which on
/// e.g. robostack (34k names) takes minutes. The GR channels are all small —
/// `general` is 283 packages, the product channels 1-2 each.
struct ChannelIndex {
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
    /// Sweep `channel_url` for `target_platform`. An *empty* channel yields an
    /// empty index — "nothing is published yet". An *unreachable* one is an
    /// error: an index that wrongly looks empty makes every package look
    /// unpublished.
    fn sweep(channel_url: &str, target_platform: TargetPlatform) -> anyhow::Result<Self> {
        let index = Self::from_records(
            &search_channel("*", channel_url, target_platform)?.unwrap_or_default(),
        );
        tracing::info!(
            "swept {channel_url} for {}: {} name/version pairs across {} subdir slots",
            target_platform.arch(),
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
    fn has_build(&self, name: &str, version: &str, build_number: u64, subdir: BuildSubdir) -> bool {
        self.builds
            .get(&(subdir.to_string(), name.to_string(), version.to_string()))
            .is_some_and(|b| b.contains(&build_number))
    }

    /// Whether *any* build of `name == version` is published, in any subdir.
    /// For dep satisfaction we only care that some build of the pinned version
    /// is available to solve against.
    fn has_version(&self, name: &str, version: &str) -> bool {
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
struct ChannelIndexCache {
    target_platform: TargetPlatform,
    // ponytail: one lock over the whole map, held across the sweep, so sweeps
    // of *different* channels don't overlap. Deliberate — it also means a
    // channel is never swept twice concurrently, and the totals are small
    // (~23 channels for ros-recipes, product channels hold 1-2 packages and
    // sweep in ~0.5s). Go per-channel locks if the channel count grows enough
    // that serialised cold sweeps start to show.
    swept: Mutex<HashMap<String, Arc<ChannelIndex>>>,
}

impl ChannelIndexCache {
    fn new(target_platform: TargetPlatform) -> Self {
        Self {
            target_platform,
            swept: Mutex::new(HashMap::new()),
        }
    }

    /// The snapshot for `channel_url`, sweeping it if this is the first ask.
    fn get(&self, channel_url: &str) -> anyhow::Result<Arc<ChannelIndex>> {
        let mut swept = self.swept.lock().expect("channel index cache poisoned");
        if let Some(index) = swept.get(channel_url) {
            return Ok(Arc::clone(index));
        }
        let index = Arc::new(ChannelIndex::sweep(channel_url, self.target_platform)?);
        swept.insert(channel_url.to_string(), Arc::clone(&index));
        Ok(index)
    }
}

/// Whether *any* build of `name == version` exists in `channel_url` for
/// `target_platform`, asked live.
///
/// For the local `file://` channels only: those gain packages as the job
/// publishes into them, so a snapshot would go stale mid-loop. Their repodata
/// is small and uncontended, so the per-package search is cheap here. Use
/// [`ChannelIndex`] for the upstream channel instead.
fn version_published(
    name: &str,
    version: &str,
    channel_url: &str,
    target_platform: TargetPlatform,
) -> anyhow::Result<bool> {
    let spec = format!("{name}=={version}");
    let Some(parsed) = search_channel(&spec, channel_url, target_platform)? else {
        return Ok(false);
    };
    Ok(parsed
        .values()
        .flatten()
        .any(|r| r.name == name && r.version == version))
}

enum CheckOutcome {
    Build {
        name: String,
        version: String,
        upstream_build: u64,
        effective_build: u64,
        upstream: Box<UpstreamPixiToml>,
    },
    SkipPlatformUnsupported {
        name: String,
        version: String,
    },
    SkipNoarchNonCanonical {
        name: String,
        version: String,
    },
    SkipAlreadyPublished {
        name: String,
        version: String,
        channels: Vec<String>,
    },
}

fn check_entry(
    entry: &PixiNativeEntry,
    channels: &ChannelIndexCache,
    channel_url: &str,
    routing_rules: &[crate::routing::RoutingRule],
    target_platform: TargetPlatform,
    rebuild_epoch: u64,
) -> anyhow::Result<CheckOutcome> {
    let pixi_toml_text = fetch_pixi_toml(entry)?;
    let upstream = UpstreamPixiToml::parse(&pixi_toml_text)
        .with_context(|| format!("entry {}: parse upstream pixi.toml", entry.name))?;

    if !upstream.supports_platform(target_platform) {
        return Ok(CheckOutcome::SkipPlatformUnsupported {
            name: upstream.package.name,
            version: upstream.package.version,
        });
    }

    // noarch artifacts are arch-independent; build them once on linux-64 and
    // skip every other platform rather than repeating the (identical) build.
    if upstream.is_noarch() && target_platform.arch() != Arch::Linux64 {
        return Ok(CheckOutcome::SkipNoarchNonCanonical {
            name: upstream.package.name,
            version: upstream.package.version,
        });
    }

    let upstream_build = upstream.build_number();
    let effective_build = upstream_build + rebuild_epoch;

    // Routed packages (see routing.yaml) publish to product channels, never
    // to the default channel — search where they actually land. Skip only
    // when every routed channel has the build (a partially-drained
    // multi-channel publish, e.g. dual-publish rules, should re-run).
    let published_urls = crate::routing::published_channel_urls(
        routing_rules,
        channel_url,
        &upstream.package.name,
        &upstream.package.version,
    );
    let subdir = BuildSubdir::of(&upstream, target_platform);
    let mut published_everywhere = true;
    for url in &published_urls {
        published_everywhere &= channels.get(url)?.has_build(
            &upstream.package.name,
            &upstream.package.version,
            effective_build,
            subdir,
        );
    }
    if published_everywhere {
        return Ok(CheckOutcome::SkipAlreadyPublished {
            name: upstream.package.name,
            version: upstream.package.version,
            channels: published_urls,
        });
    }

    Ok(CheckOutcome::Build {
        name: upstream.package.name.clone(),
        version: upstream.package.version.clone(),
        upstream_build,
        effective_build,
        upstream: Box::new(upstream),
    })
}

/// Rewrite `[package.build.config].build-number` in the given `pixi.toml`
/// to `value`, creating the intermediate tables if absent. Preserves
/// comments and formatting of the rest of the file.
fn rewrite_build_number(manifest_path: &Path, value: u64) -> anyhow::Result<()> {
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {} as TOML", manifest_path.display()))?;

    let package = doc
        .get_mut("package")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| anyhow::anyhow!("{}: missing [package] table", manifest_path.display(),))?;

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

    fs::write(manifest_path, doc.to_string())
        .with_context(|| format!("write {}", manifest_path.display()))?;
    Ok(())
}

/// Select entries to build: keep those matching `runner_size` (when set) and,
/// when `only` is non-empty, only those whose name is listed.
fn select_entries<'a>(
    packages: &'a [PixiNativeEntry],
    runner_size: Option<RunnerSize>,
    only: &[String],
) -> Vec<&'a PixiNativeEntry> {
    packages
        .iter()
        .filter(|e| runner_size.is_none_or(|s| e.runner_size == s))
        .filter(|e| only.is_empty() || only.iter().any(|n| n == &e.name))
        .collect()
}

fn pixi(
    repo_root: Option<PathBuf>,
    channel_url: String,
    output_dir: PathBuf,
    target_platform: TargetPlatform,
    runner_size: Option<RunnerSize>,
    only: &[String],
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

    let channel_url_ref: &str = &channel_url;
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
                            channel_url_ref,
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
                    name: upstream.package.name.clone(),
                    rel_path_deps: upstream.path_dep_rel_paths(),
                    // Dep keys of exact `==X.Y.Z` pins. Mirrors `rel_path_deps`
                    // above for the committed-pin farm ordering rule.
                    pin_dep_names: upstream.exact_pins().into_iter().map(|(k, _)| k).collect(),
                });
            }
            CheckOutcome::SkipPlatformUnsupported { name, version } => {
                tracing::info!(
                    "skipping {name} {version}: pixi.toml does not list {}",
                    target_platform.arch(),
                );
            }
            CheckOutcome::SkipNoarchNonCanonical { name, version } => {
                tracing::info!(
                    "skipping {name} {version}: noarch, built only on linux-64 (not {})",
                    target_platform.arch(),
                );
            }
            CheckOutcome::SkipAlreadyPublished {
                name,
                version,
                channels,
            } => {
                tracing::info!(
                    "skipping {name} {version}: already in channel(s) {}",
                    channels.join(", "),
                );
            }
        }
    }

    if to_build.is_empty() {
        tracing::info!("nothing to build");
        return Ok(());
    }

    tracing::info!(
        "building {} entries: {}",
        to_build.len(),
        build_labels.join(", "),
    );

    let to_build = topo_sort_builds(to_build)?;

    // Dep satisfaction deliberately only consults the default channel, same as
    // before routing was introduced: routing decides where an artifact is
    // *published*, not which channels a consumer solves against.
    let default_channel = channels.get(&channel_url)?;

    let mut built_this_job: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for item in to_build {
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

        let subdir = entry.subdir.as_deref().unwrap_or(Path::new("."));
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
            rewrite_build_number(&manifest_path, effective_build).with_context(|| {
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
        let mut extra_channels: Vec<String> = Vec::new();
        let mut local_built: BTreeSet<String> = BTreeSet::new();
        let mut visiting: Vec<String> = Vec::new();
        let local_ctx = LocalBuildCtx {
            local_deps_dir: &local_deps_dir,
            channel: &default_channel,
            target_platform,
            workdir: &workdir,
            sibling_subdirs: &sib_subdirs,
        };
        for dep in &resolved {
            let output_channel = format!("file://{}", abs_output.display());
            // The output channel gains packages as this loop publishes into it,
            // so it has to be asked live rather than swept.
            let in_output_channel = built_this_job.contains(&dep.name)
                || version_published(&dep.name, &dep.version, &output_channel, target_platform)?;
            if in_output_channel {
                push_unique(&mut extra_channels, output_channel);
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
                    format!("file://{}", local_deps_dir.display()),
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
        let target_channel = format!("file://{}", abs_output.display());
        let arch = target_platform.arch().to_string();
        process::run(
            "pixi",
            &publish_argv(&manifest_path, &target_channel, &arch),
        )?;
        built_this_job.insert(item.name.clone());
    }

    Ok(())
}

fn deepstream_container(
    repo_root: Option<PathBuf>,
    channel_url: String,
    output_dir: PathBuf,
    target_platform: TargetPlatform,
    ds_recipes: Vec<RecipeName>,
    ds_version: DeepstreamVersion,
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;

    // 1. Configure git HTTPS auth for private repo clones. The container
    //    cannot inherit the runner's git config, so this has to happen here.
    gh::ensure_git_auth()?;

    // 2. Host's .pixi/ has host-absolute shebangs that fail in-container; rebuild.
    let host_pixi = repo.root().join(".pixi");
    if host_pixi.exists() {
        fs::remove_dir_all(&host_pixi)
            .with_context(|| format!("remove {}", host_pixi.display()))?;
    }
    // Workaround for stale-partial-entry errors during run_exports collection.
    for cache in [
        "/tmp/.cache/rattler",
        &format!(
            "{}/.cache/rattler",
            std::env::var("HOME").unwrap_or_default()
        ),
    ] {
        let p = Path::new(cache);
        if p.exists() {
            // Best-effort: ignore failures cleaning caches.
            let _ = fs::remove_dir_all(p);
        }
    }

    // 3. Install pixi env for the repo.
    process::run_in(repo.root(), "pixi", &["install"])?;

    // 4. Delegate to build vinca (always DeepstreamOnly mode). DeepStream
    // builds are filtered to DS recipes only and never touch overrides
    // packages, so no overrides channel is passed.
    vinca(
        Some(repo.root().to_path_buf()),
        channel_url,
        None,
        output_dir,
        target_platform,
        ds_recipes,
        Some(ds_version),
        Vec::new(),
    )?;

    Ok(())
}

#[cfg(test)]
#[path = "build_recipes_tests.rs"]
mod tests;
