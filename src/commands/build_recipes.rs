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
    let _pin = if let Some(v) = mode_version(&mode) {
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
        "https://prefix.dev/robostack-kilted",
        "-c",
        "https://prefix.dev/conda-forge",
        "--skip-existing=all",
        "--output-dir",
        &abs_output_str,
    ]);
    process::run_in(repo.root(), "pixi", &args)?;

    Ok(())
}

fn mode_version(mode: &VincaBuildMode) -> Option<DeepstreamVersion> {
    match mode {
        VincaBuildMode::DeepstreamOnly { version, .. } => Some(*version),
        VincaBuildMode::Only { version, .. } => *version,
        _ => None,
    }
}

use anyhow::Context;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

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
}

/// A sibling package whose `path =` dep was rewritten to a derived `==version`
/// pin in the temp checkout.
#[derive(Debug)]
pub struct ResolvedDep {
    /// The dependency *key* in the consumer's manifest (e.g. `ros-kilted-lib`).
    /// This is the channel artifact name, which is what `version_published` /
    /// `built_this_job` / the local-build guard key off of. It is NOT
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

/// Rewrite `path =` deps in a single table-like section (e.g. `[dependencies]`
/// or `[package.run-dependencies]`) to `"==<version>"`, skipping the
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
        let sib_manifest = manifest_dir.join(path).join("pixi.toml");
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
        table.insert(&key, toml_edit::value(format!("=={version}")));
        resolved.push(ResolvedDep {
            name: key.clone(),
            version,
            manifest: sib_manifest,
        });
    }
    Ok(())
}

/// Rewrite every non-self `path =` dep in the temp-checkout manifest to
/// `"==<version>"`, reading the version from the sibling manifest at the same
/// rev. The derived pin is deterministic: same rev -> same sibling manifest
/// -> same pin. Committed manifests are never touched.
fn resolve_path_deps(manifest_path: &Path) -> anyhow::Result<Vec<ResolvedDep>> {
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

/// Build a path-dep sibling from the consumer's checkout into a local-only
/// file channel. Recurses through the sibling's own path deps first.
/// `local_built` and `visiting` are scoped per top-level entry (fresh at each
/// call site in the main build loop): `local_built` dedupes a diamond
/// dependency shared by two siblings, and `visiting` catches a cycle among
/// path deps before it reaches pixi's solver.
/// ponytail: duplicate build when the sibling lives in another matrix job;
/// layered farm stages are the upgrade path if this gets slow in practice.
fn build_local_dep(
    dep: &ResolvedDep,
    local_deps_dir: &Path,
    channel_url: &str,
    target_platform: TargetPlatform,
    local_built: &mut BTreeSet<String>,
    visiting: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !check_local_build_guard(&dep.name, visiting, local_built)? {
        return Ok(());
    }
    fs::create_dir_all(local_deps_dir)?;
    visiting.push(dep.name.clone());
    let nested = resolve_path_deps(&dep.manifest)?;
    for n in &nested {
        if !version_published(&n.name, &n.version, channel_url, target_platform) {
            build_local_dep(
                n,
                local_deps_dir,
                channel_url,
                target_platform,
                local_built,
                visiting,
            )?;
        }
    }
    visiting.pop();
    if !nested.is_empty() {
        prepend_channels(
            &dep.manifest,
            &[format!("file://{}", local_deps_dir.display())],
        )?;
    }
    let target_channel = format!("file://{}", local_deps_dir.display());
    process::run(
        "pixi",
        &[
            "publish",
            "--path",
            dep.manifest.to_str().unwrap(),
            "--target-channel",
            &target_channel,
            "--target-platform",
            &target_platform.arch().to_string(),
        ],
    )?;
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
}

/// Order build items so same-repo path-dep targets build before consumers.
/// Edge rule: consumer.subdir/rel_path (normalized) == target.subdir, same url.
fn topo_sort_builds(items: Vec<BuildItem<'_>>) -> anyhow::Result<Vec<BuildItem<'_>>> {
    use crate::commands::ci::siblings::normalize;
    use std::collections::BTreeMap;

    let key = |e: &PixiNativeEntry| {
        (
            format!("{}/{}", e.url.owner, e.url.repo),
            normalize(e.subdir.as_deref().unwrap_or(Path::new("."))),
        )
    };
    let index: BTreeMap<_, usize> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (key(it.entry), i))
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

/// Fetch `pixi.toml` for an entry from `raw.githubusercontent.com`. Uses Bearer
/// auth from `GITHUB_TOKEN` / `GH_TOKEN` when present.
fn fetch_pixi_toml(entry: &PixiNativeEntry) -> anyhow::Result<String> {
    let subdir = entry
        .subdir
        .as_deref()
        .map(|p| p.to_string_lossy().trim_matches('/').to_string())
        .filter(|s| !s.is_empty() && s != ".")
        .map(|s| format!("{s}/pixi.toml"))
        .unwrap_or_else(|| "pixi.toml".to_string());

    let raw_url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        entry.url.owner,
        entry.url.repo,
        entry.rev.as_str(),
        subdir
    );

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok();

    let mut req = ureq::get(&raw_url);
    if let Some(t) = &token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    match req.call() {
        Ok(resp) => resp
            .into_string()
            .with_context(|| format!("read body of {raw_url}")),
        Err(ureq::Error::Status(code, _)) => {
            let hint = if token.is_none() && (code == 401 || code == 403 || code == 404) {
                " (set GITHUB_TOKEN for private repos)"
            } else {
                ""
            };
            anyhow::bail!(
                "entry {}: failed to fetch {raw_url} ({code}){hint}",
                entry.name
            )
        }
        Err(e) => anyhow::bail!("entry {}: fetch {raw_url}: {e}", entry.name),
    }
}

/// Initialize a fresh git repo in `dest`, fetch the given commit, and check it out.
fn fetch_at_rev(url: &GithubRepoUrl, rev: &Sha40, dest: &Path) -> anyhow::Result<()> {
    let url_str = format!("https://github.com/{}/{}", url.owner, url.repo);

    process::git(&["init", "--quiet", dest.to_str().unwrap()])?;
    process::git(&[
        "-C",
        dest.to_str().unwrap(),
        "remote",
        "add",
        "origin",
        &url_str,
    ])?;
    process::git(&[
        "-C",
        dest.to_str().unwrap(),
        "fetch",
        "--depth=1",
        "--quiet",
        "origin",
        rev.as_str(),
    ])?;
    process::git(&[
        "-C",
        dest.to_str().unwrap(),
        "checkout",
        "--quiet",
        "FETCH_HEAD",
    ])?;
    Ok(())
}

use std::process::Command;

/// Check whether `name == version` (with `build_number`) is already in `channel_url`
/// for `target_platform`. Returns `false` on any failure (the caller logs and proceeds
/// as if not published).
fn package_published(
    name: &str,
    version: &str,
    build_number: u64,
    channel_url: &str,
    target_platform: TargetPlatform,
) -> bool {
    let arch = target_platform.arch().to_string();
    let pkg_spec = format!("{name}=={version}");

    let output = Command::new("pixi")
        .args([
            "search",
            "--json",
            &pkg_spec,
            "-c",
            channel_url,
            "-p",
            &arch,
        ])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            tracing::info!("pixi search for {pkg_spec} failed to spawn: {e}");
            return false;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::info!(
            "pixi search for {pkg_spec} exited {}: {}",
            output.status,
            stderr.trim(),
        );
        return false;
    }

    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            tracing::info!("pixi search for {pkg_spec} returned non-JSON stdout: {e}");
            return false;
        }
    };

    let Some(candidates) = parsed.get(&arch).and_then(|v| v.as_array()) else {
        return false;
    };
    tracing::info!(
        "pixi search for {pkg_spec} build={build_number} on {arch}: {} candidate(s)",
        candidates.len(),
    );
    for pkg in candidates {
        let pkg_name = pkg.get("name").and_then(|v| v.as_str());
        let pkg_ver = pkg.get("version").and_then(|v| v.as_str());
        let pkg_build = pkg.get("build_number").and_then(|v| v.as_u64());
        if pkg_name == Some(name) && pkg_ver == Some(version) && pkg_build == Some(build_number) {
            return true;
        }
    }
    false
}

/// Check whether *any* build of `name == version` exists in `channel_url` for
/// `target_platform`. Same `pixi search --json` call as `package_published`,
/// but without the build-number equality check — for dep satisfaction we only
/// care that some build of the pinned version is available. Returns `false`
/// on any failure (the caller treats that as not-yet-published).
fn version_published(
    name: &str,
    version: &str,
    channel_url: &str,
    target_platform: TargetPlatform,
) -> bool {
    let arch = target_platform.arch().to_string();
    let pkg_spec = format!("{name}=={version}");

    let output = Command::new("pixi")
        .args([
            "search",
            "--json",
            &pkg_spec,
            "-c",
            channel_url,
            "-p",
            &arch,
        ])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            tracing::info!("pixi search for {pkg_spec} failed to spawn: {e}");
            return false;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::info!(
            "pixi search for {pkg_spec} exited {}: {}",
            output.status,
            stderr.trim(),
        );
        return false;
    }

    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            tracing::info!("pixi search for {pkg_spec} returned non-JSON stdout: {e}");
            return false;
        }
    };

    let Some(candidates) = parsed.get(&arch).and_then(|v| v.as_array()) else {
        return false;
    };
    candidates.iter().any(|pkg| {
        pkg.get("name").and_then(|v| v.as_str()) == Some(name)
            && pkg.get("version").and_then(|v| v.as_str()) == Some(version)
    })
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
    },
}

fn check_entry(
    entry: &PixiNativeEntry,
    channel_url: &str,
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

    if package_published(
        &upstream.package.name,
        &upstream.package.version,
        effective_build,
        channel_url,
        target_platform,
    ) {
        return Ok(CheckOutcome::SkipAlreadyPublished {
            name: upstream.package.name,
            version: upstream.package.version,
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

    let abs_output = if output_dir.is_absolute() {
        output_dir
    } else {
        repo.root().join(&output_dir)
    };
    fs::create_dir_all(&abs_output).with_context(|| format!("mkdir {}", abs_output.display()))?;

    if manifest.packages.is_empty() {
        tracing::info!("pixi_native_packages.yaml has no entries; nothing to build");
        return Ok(());
    }

    let filtered = select_entries(&manifest.packages, runner_size, only);

    if filtered.is_empty() {
        return Ok(());
    }

    let channel_url_ref: &str = &channel_url;
    let rebuild_epoch = manifest.rebuild_epoch;
    let outcomes: Vec<(&PixiNativeEntry, anyhow::Result<CheckOutcome>)> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = filtered
                .iter()
                .copied()
                .map(|entry| {
                    scope.spawn(move || {
                        check_entry(entry, channel_url_ref, target_platform, rebuild_epoch)
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
            CheckOutcome::SkipAlreadyPublished { name, version } => {
                tracing::info!("skipping {name} {version}: already in channel {channel_url}");
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
        let manifest_path = manifest_dir.join("pixi.toml");
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
        let resolved = resolve_path_deps(&manifest_path)?;
        let mut extra_channels: Vec<String> = Vec::new();
        let mut local_built: BTreeSet<String> = BTreeSet::new();
        let mut visiting: Vec<String> = Vec::new();
        for dep in &resolved {
            if built_this_job.contains(&dep.name) {
                push_unique(
                    &mut extra_channels,
                    format!("file://{}", abs_output.display()),
                );
            } else if version_published(&dep.name, &dep.version, &channel_url, target_platform) {
                // Satisfied by the real channel; nothing to do.
            } else {
                // Fallback: build the sibling from this same checkout (correct
                // rev by construction) into a local-only channel. Not drained;
                // the sibling's own entry / linux-64 stays the canonical publisher.
                tracing::info!(
                    "entry {}: sibling {} =={} not in channel and not built this job; fallback local build",
                    entry.name,
                    dep.name,
                    dep.version,
                );
                build_local_dep(
                    dep,
                    &local_deps_dir,
                    &channel_url,
                    target_platform,
                    &mut local_built,
                    &mut visiting,
                )?;
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
            &[
                "publish",
                "--path",
                manifest_path.to_str().unwrap(),
                "--target-channel",
                &target_channel,
                "--target-platform",
                &arch,
            ],
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

    // 1. Configure git to use GH_TOKEN for HTTPS auth (private repo clones).
    if let Ok(token) = std::env::var("GH_TOKEN") {
        let insteadof = format!("url.https://x-access-token:{token}@github.com/.insteadOf");
        process::git(&["config", "--global", &insteadof, "https://github.com/"])?;
    }

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
mod tests {
    use super::*;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn recipe(name: &str) -> RecipeName {
        RecipeName::from_str(name).unwrap()
    }

    fn is_noarch(toml: &str) -> bool {
        UpstreamPixiToml::parse(toml).unwrap().is_noarch()
    }

    fn test_entry(name: &str, subdir: &str) -> PixiNativeEntry {
        PixiNativeEntry {
            name: name.into(),
            url: GithubRepoUrl::parse("https://github.com/gr/repo").unwrap(),
            rev: Sha40::new("a".repeat(40)).unwrap(),
            subdir: Some(PathBuf::from(subdir)),
            runner_size: RunnerSize::default(),
        }
    }

    #[test]
    fn path_dep_rel_paths_excludes_self_idiom() {
        let t = UpstreamPixiToml::parse(
            "[dependencies]\nnode = { path = \".\" }\n\
             [package]\nname=\"node\"\nversion=\"1.0.0\"\n\
             [package.run-dependencies]\nlib = { path = \"../lib\" }\nros-kilted-rclpy = \"*\"\n\
             [package.host-dependencies]\nmsgs = { path = \"../msgs\" }\n",
        )
        .unwrap();
        let mut got = t.path_dep_rel_paths();
        got.sort();
        assert_eq!(got, vec!["../lib".to_string(), "../msgs".to_string()]);
    }

    #[test]
    fn topo_sort_builds_orders_same_repo_path_deps() {
        let lib = test_entry("lib", "packages/lib");
        let node = test_entry("node", "packages/node");
        let items = vec![
            BuildItem {
                entry: &node,
                effective_build: 0,
                name: "node".into(),
                rel_path_deps: vec!["../lib".into()],
            },
            BuildItem {
                entry: &lib,
                effective_build: 0,
                name: "lib".into(),
                rel_path_deps: vec![],
            },
        ];
        let sorted = topo_sort_builds(items).unwrap();
        assert_eq!(sorted[0].name, "lib");
        assert_eq!(sorted[1].name, "node");
    }

    #[test]
    fn topo_sort_builds_rejects_cycles() {
        let a = test_entry("a", "packages/a");
        let b = test_entry("b", "packages/b");
        let items = vec![
            BuildItem {
                entry: &a,
                effective_build: 0,
                name: "a".into(),
                rel_path_deps: vec!["../b".into()],
            },
            BuildItem {
                entry: &b,
                effective_build: 0,
                name: "b".into(),
                rel_path_deps: vec!["../a".into()],
            },
        ];
        assert!(
            topo_sort_builds(items)
                .unwrap_err()
                .to_string()
                .contains("cycle")
        );
    }

    #[test]
    fn noarch_detects_python_and_ament_python() {
        // pixi-build-python backend → noarch
        assert!(is_noarch(
            "[package]\nname=\"c\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-python\"\nversion=\"*\"",
        ));
        // ament_python ROS package → noarch
        assert!(is_noarch(
            "[package]\nname=\"p\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-ros-gr\"\n\
             [package.build.config]\nbuild-type=\"ament_python\"",
        ));
    }

    #[test]
    fn noarch_false_for_compiled_and_missing_build() {
        // ament_cmake / ament_idl compile per-arch → not noarch
        for bt in ["ament_cmake", "ament_idl", "cmake"] {
            assert!(
                !is_noarch(&format!(
                    "[package]\nname=\"x\"\nversion=\"1\"\n\
                     [package.build.backend]\nname=\"pixi-build-ros-gr\"\n\
                     [package.build.config]\nbuild-type=\"{bt}\"",
                )),
                "expected {bt} to be arch-specific"
            );
        }
        // no [package.build] at all → conservative: not noarch
        assert!(!is_noarch("[package]\nname=\"x\"\nversion=\"1\""));
    }

    #[test]
    fn vinca_mode_normal_when_no_flags() {
        let m = VincaBuildMode::from_flags(vec![], None, vec![]).unwrap();
        assert_eq!(m, VincaBuildMode::Normal);
    }

    #[test]
    fn vinca_mode_drop_when_only_recipes() {
        let m = VincaBuildMode::from_flags(vec![recipe("a"), recipe("b")], None, vec![]).unwrap();
        assert_eq!(
            m,
            VincaBuildMode::DropDeepstream {
                recipes: vec![recipe("a"), recipe("b")]
            }
        );
    }

    #[test]
    fn vinca_mode_deepstream_only_when_ds_recipe_and_version() {
        let m =
            VincaBuildMode::from_flags(vec![recipe("a")], Some(DeepstreamVersion::V7_1), vec![])
                .unwrap();
        assert_eq!(
            m,
            VincaBuildMode::DeepstreamOnly {
                recipes: vec![recipe("a")],
                version: DeepstreamVersion::V7_1,
            }
        );
    }

    #[test]
    fn vinca_mode_rejects_version_without_recipes() {
        let err =
            VincaBuildMode::from_flags(vec![], Some(DeepstreamVersion::V8_0), vec![]).unwrap_err();
        assert!(format!("{err:#}").contains("requires at least one --ds-recipe or --only"));
    }

    #[test]
    fn vinca_mode_only_alone_unpinned() {
        let m = VincaBuildMode::from_flags(vec![], None, vec![recipe("foo")]).unwrap();
        assert_eq!(
            m,
            VincaBuildMode::Only {
                recipes: vec![recipe("foo")],
                version: None,
            }
        );
    }

    #[test]
    fn vinca_mode_only_with_ds_version_pins() {
        let m =
            VincaBuildMode::from_flags(vec![], Some(DeepstreamVersion::V8_0), vec![recipe("foo")])
                .unwrap();
        assert_eq!(
            m,
            VincaBuildMode::Only {
                recipes: vec![recipe("foo")],
                version: Some(DeepstreamVersion::V8_0),
            }
        );
    }

    #[test]
    fn vinca_mode_only_rejects_combined_with_ds_recipe() {
        let err =
            VincaBuildMode::from_flags(vec![recipe("a")], None, vec![recipe("b")]).unwrap_err();
        assert!(format!("{err:#}").contains("mutually exclusive"));
    }

    fn write_recipe_dir(parent: &Path, name: &str) {
        let d = parent.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("recipe.yaml"), "marker").unwrap();
    }

    fn make_repo_with_recipes(recipe_names: &[&str], vendor_names: &[&str]) -> TempDir {
        let td = TempDir::new().unwrap();
        let recipes = td.path().join("recipes");
        fs::create_dir_all(&recipes).unwrap();
        for n in recipe_names {
            write_recipe_dir(&recipes, n);
        }
        if !vendor_names.is_empty() {
            let vendor = td.path().join("vendor_recipes");
            fs::create_dir_all(&vendor).unwrap();
            for n in vendor_names {
                write_recipe_dir(&vendor, n);
            }
        }
        td
    }

    fn recipe_names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().unwrap().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn apply_filter_overlays_vendor_recipes() {
        let td = make_repo_with_recipes(&["foo"], &["bar"]);
        apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
        assert_eq!(
            recipe_names_in(&td.path().join("recipes")),
            vec!["bar", "foo"]
        );
    }

    #[test]
    fn apply_filter_overlays_vendor_overwriting() {
        // recipes/foo exists with content "marker"; vendor_recipes/foo exists too
        let td = TempDir::new().unwrap();
        let recipes = td.path().join("recipes");
        fs::create_dir_all(&recipes).unwrap();
        write_recipe_dir(&recipes, "foo");
        fs::write(recipes.join("foo/old.txt"), "old").unwrap();
        let vendor = td.path().join("vendor_recipes");
        fs::create_dir_all(&vendor).unwrap();
        write_recipe_dir(&vendor, "foo");
        fs::write(vendor.join("foo/new.txt"), "new").unwrap();

        apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
        // After overlay, recipes/foo/new.txt should exist and recipes/foo/old.txt should not.
        assert!(recipes.join("foo/new.txt").exists());
        assert!(!recipes.join("foo/old.txt").exists());
    }

    #[test]
    fn apply_filter_removes_deepstream_mutex() {
        let td = make_repo_with_recipes(&["foo", "deepstream-mutex"], &[]);
        apply_recipe_filter(td.path(), &VincaBuildMode::Normal).unwrap();
        assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
    }

    #[test]
    fn apply_filter_drop_deepstream_removes_listed() {
        let td = make_repo_with_recipes(&["foo", "deepstream-a", "deepstream-b"], &[]);
        apply_recipe_filter(
            td.path(),
            &VincaBuildMode::DropDeepstream {
                recipes: vec![recipe("deepstream-a"), recipe("deepstream-b")],
            },
        )
        .unwrap();
        assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
    }

    #[test]
    fn apply_filter_deepstream_only_keeps_listed() {
        let td = make_repo_with_recipes(&["foo", "deepstream-a", "deepstream-b"], &[]);
        apply_recipe_filter(
            td.path(),
            &VincaBuildMode::DeepstreamOnly {
                recipes: vec![recipe("deepstream-a")],
                version: DeepstreamVersion::V7_1,
            },
        )
        .unwrap();
        assert_eq!(
            recipe_names_in(&td.path().join("recipes")),
            vec!["deepstream-a"]
        );
    }

    #[test]
    fn apply_filter_only_keeps_listed_regardless_of_ds() {
        let td = make_repo_with_recipes(&["foo", "bar", "deepstream-a"], &[]);
        apply_recipe_filter(
            td.path(),
            &VincaBuildMode::Only {
                recipes: vec![recipe("foo")],
                version: None,
            },
        )
        .unwrap();
        assert_eq!(recipe_names_in(&td.path().join("recipes")), vec!["foo"]);
    }

    #[test]
    fn write_variants_pin_v71_pins_gcc_13() {
        let tf = write_variants_pin(DeepstreamVersion::V7_1).unwrap();
        let content = fs::read_to_string(tf.path()).unwrap();
        assert!(
            content.contains("deepstream_version:\n  - \"7.1\""),
            "got: {content}"
        );
        assert!(
            content.contains("c_compiler_version:\n  - \"13\""),
            "got: {content}"
        );
        assert!(
            content.contains("cxx_compiler_version:\n  - \"13\""),
            "got: {content}"
        );
    }

    #[test]
    fn write_variants_pin_v80_no_compiler_pin() {
        let tf = write_variants_pin(DeepstreamVersion::V8_0).unwrap();
        let content = fs::read_to_string(tf.path()).unwrap();
        assert!(
            content.contains("deepstream_version:\n  - \"8.0\""),
            "got: {content}"
        );
        assert!(
            !content.contains("c_compiler_version"),
            "should not pin gcc for 8.0: {content}"
        );
    }

    #[test]
    fn mode_version_extracts_from_deepstream_only() {
        assert_eq!(mode_version(&VincaBuildMode::Normal), None);
        assert_eq!(
            mode_version(&VincaBuildMode::DropDeepstream {
                recipes: vec![recipe("a")]
            }),
            None,
        );
        assert_eq!(
            mode_version(&VincaBuildMode::DeepstreamOnly {
                recipes: vec![recipe("a")],
                version: DeepstreamVersion::V8_0,
            }),
            Some(DeepstreamVersion::V8_0),
        );
        assert_eq!(
            mode_version(&VincaBuildMode::Only {
                recipes: vec![recipe("a")],
                version: None,
            }),
            None,
        );
        assert_eq!(
            mode_version(&VincaBuildMode::Only {
                recipes: vec![recipe("a")],
                version: Some(DeepstreamVersion::V7_1),
            }),
            Some(DeepstreamVersion::V7_1),
        );
    }

    #[test]
    fn upstream_pixi_toml_parses_minimal() {
        let text = r#"
[package]
name = "foo"
version = "1.2.3"
"#;
        let p = UpstreamPixiToml::parse(text).unwrap();
        assert_eq!(p.package.name, "foo");
        assert_eq!(p.package.version, "1.2.3");
        assert_eq!(p.build_number(), 0);
    }

    #[test]
    fn upstream_pixi_toml_parses_build_number() {
        let text = r#"
[package]
name = "foo"
version = "1.2.3"

[package.build.config]
build-number = 5
"#;
        let p = UpstreamPixiToml::parse(text).unwrap();
        assert_eq!(p.build_number(), 5);
    }

    #[test]
    fn upstream_pixi_toml_supports_platform_when_empty() {
        let text = r#"
[package]
name = "foo"
version = "1.0"
"#;
        let p = UpstreamPixiToml::parse(text).unwrap();
        assert!(p.supports_platform(TargetPlatform::default()));
    }

    #[test]
    fn upstream_pixi_toml_respects_platforms_list() {
        let text = r#"
[package]
name = "foo"
version = "1.0"

[workspace]
platforms = ["linux-64"]
"#;
        let p = UpstreamPixiToml::parse(text).unwrap();
        assert!(p.supports_platform(TargetPlatform::default()));
        let aarch = TargetPlatform::from_str("linux-aarch64").unwrap();
        assert!(!p.supports_platform(aarch));
    }

    #[test]
    fn upstream_pixi_toml_ignores_unknown_keys() {
        let text = r#"
[package]
name = "foo"
version = "1.0"

[tasks]
ci = "test"

[dependencies]
something = "1"
"#;
        UpstreamPixiToml::parse(text).unwrap();
    }

    fn write_tmp_pixi_toml(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("pixi.toml");
        std::fs::write(&path, text).unwrap();
        (tmp, path)
    }

    #[test]
    fn rewrite_build_number_updates_existing_field() {
        let original = r#"[package]
name = "foo"
version = "1.0"

[package.build.config]
build-number = 0
"#;
        let (_tmp, path) = write_tmp_pixi_toml(original);
        rewrite_build_number(&path, 5).unwrap();
        let updated = std::fs::read_to_string(&path).unwrap();
        let reparsed = UpstreamPixiToml::parse(&updated).unwrap();
        assert_eq!(reparsed.build_number(), 5);
    }

    #[test]
    fn rewrite_build_number_inserts_when_absent() {
        let original = r#"[package]
name = "foo"
version = "1.0"
"#;
        let (_tmp, path) = write_tmp_pixi_toml(original);
        rewrite_build_number(&path, 2).unwrap();
        let updated = std::fs::read_to_string(&path).unwrap();
        let reparsed = UpstreamPixiToml::parse(&updated).unwrap();
        assert_eq!(reparsed.build_number(), 2);
    }

    #[test]
    fn rewrite_build_number_is_idempotent() {
        let original = r#"[package]
name = "foo"
version = "1.0"

[package.build.config]
build-number = 0
"#;
        let (_tmp, path) = write_tmp_pixi_toml(original);
        rewrite_build_number(&path, 4).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        rewrite_build_number(&path, 4).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn rewrite_build_number_preserves_unrelated_keys_and_comments() {
        let original = r#"# top-of-file comment
[package]
name = "foo"  # inline comment
version = "1.0"

[tasks]
ci = "test"
"#;
        let (_tmp, path) = write_tmp_pixi_toml(original);
        rewrite_build_number(&path, 1).unwrap();
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# top-of-file comment"), "got: {updated}");
        assert!(updated.contains("# inline comment"), "got: {updated}");
        assert!(updated.contains("ci = \"test\""), "got: {updated}");
        assert!(updated.contains("build-number = 1"), "got: {updated}");
    }

    #[test]
    fn rewrite_build_number_errors_when_package_missing() {
        let original = "[tasks]\nci = \"test\"\n";
        let (_tmp, path) = write_tmp_pixi_toml(original);
        let err = rewrite_build_number(&path, 1).unwrap_err();
        assert!(format!("{err:#}").contains("missing [package]"));
    }

    fn write_checkout_pkg(root: &Path, name: &str, extra: &str) -> PathBuf {
        let dir = root.join("packages").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("pixi.toml");
        std::fs::write(
            &p,
            format!(
                "[workspace]\nname = \"{name}\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
                 [dependencies]\n{name} = {{ path = \".\" }}\n\
                 [package]\nname = \"{name}\"\nversion = \"2.5.0\"\n{extra}"
            ),
        )
        .unwrap();
        p
    }

    #[test]
    fn resolve_path_deps_rewrites_to_sibling_manifest_version() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_checkout_pkg(root, "lib", "");
        let consumer = write_checkout_pkg(
            root,
            "node",
            "[package.run-dependencies]\nlib = { path = \"../lib\" }\nros-kilted-rclpy = \"*\"\n",
        );
        let resolved = resolve_path_deps(&consumer).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "lib");
        assert_eq!(resolved[0].version, "2.5.0");

        let text = std::fs::read_to_string(&consumer).unwrap();
        assert!(text.contains("lib = \"==2.5.0\""), "rewritten: {text}");
        assert!(
            text.contains("node = { path = \".\" }"),
            "self idiom untouched: {text}"
        );
        assert!(
            text.contains("ros-kilted-rclpy"),
            "externals untouched: {text}"
        );
    }

    #[test]
    fn resolve_path_deps_uses_dep_key_not_sibling_package_name() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        // Sibling dir/manifest is named "lib" (e.g. package-xml mode: no
        // stable relationship between package.name and the channel artifact
        // name), but the consumer depends on it under the artifact-name key
        // "ros-kilted-lib".
        write_checkout_pkg(root, "lib", "");
        let consumer = write_checkout_pkg(
            root,
            "node",
            "[package.run-dependencies]\nros-kilted-lib = { path = \"../lib\" }\n",
        );
        let resolved = resolve_path_deps(&consumer).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "ros-kilted-lib");
        assert_eq!(resolved[0].version, "2.5.0");

        let text = std::fs::read_to_string(&consumer).unwrap();
        assert!(
            text.contains("ros-kilted-lib = \"==2.5.0\""),
            "rewritten under the dep key: {text}"
        );
    }

    #[test]
    fn resolve_path_deps_errors_clearly_when_sibling_has_no_version() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let dir = root.join("packages").join("lib");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pixi.toml"),
            "[workspace]\nname = \"lib\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
             [dependencies]\nlib = { path = \".\" }\n\
             [package]\nname = \"lib\"\n",
        )
        .unwrap();
        let consumer = write_checkout_pkg(
            root,
            "node",
            "[package.run-dependencies]\nlib = { path = \"../lib\" }\n",
        );
        let err = resolve_path_deps(&consumer).unwrap_err();
        assert!(
            format!("{err:#}").contains("package.version"),
            "got: {err:#}"
        );
    }

    #[test]
    fn check_local_build_guard_detects_cycle() {
        let visiting = vec!["a".to_string(), "b".to_string()];
        let local_built = BTreeSet::new();
        let err = check_local_build_guard("a", &visiting, &local_built).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cycle"), "message: {msg}");
        assert!(msg.contains("a -> b -> a"), "message: {msg}");
    }

    #[test]
    fn check_local_build_guard_skips_already_built() {
        let visiting: Vec<String> = Vec::new();
        let mut local_built = BTreeSet::new();
        local_built.insert("lib".to_string());
        assert!(!check_local_build_guard("lib", &visiting, &local_built).unwrap());
        // Not yet built and not visiting: proceed.
        assert!(check_local_build_guard("other", &visiting, &local_built).unwrap());
    }

    #[test]
    fn prepend_channels_front_inserts() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("pixi.toml");
        std::fs::write(
            &p,
            "[workspace]\nname = \"x\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
             [package]\nname = \"x\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        prepend_channels(&p, &["file:///out".into(), "file:///local-deps".into()]).unwrap();
        let doc: toml::Value = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let ch: Vec<&str> = doc["workspace"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            ch,
            vec![
                "file:///out",
                "file:///local-deps",
                "https://prefix.dev/conda-forge"
            ]
        );
    }

    #[test]
    fn select_entries_filters_by_only_and_size() {
        let yaml = r#"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
    runner-size: 4cpu
  - name: beta
    url: https://github.com/org/beta
    rev: 2222222222222222222222222222222222222222
    runner-size: 8cpu
  - name: gamma
    url: https://github.com/org/gamma
    rev: 3333333333333333333333333333333333333333
    runner-size: 4cpu
"#;
        let m = crate::types::PixiNativeManifest::from_yaml_str(yaml).unwrap();

        // --only alpha,beta with no size filter → alpha, beta
        let sel = select_entries(&m.packages, None, &["alpha".into(), "beta".into()]);
        let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);

        // --only alpha,beta + runner-size 4cpu → alpha only
        let sel = select_entries(
            &m.packages,
            Some(crate::types::RunnerSize::Cpu4),
            &["alpha".into(), "beta".into()],
        );
        let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha"]);

        // empty --only → size filter only (all 4cpu)
        let sel = select_entries(&m.packages, Some(crate::types::RunnerSize::Cpu4), &[]);
        let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "gamma"]);
    }
}
