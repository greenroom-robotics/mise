use std::path::PathBuf;

use clap::Subcommand;

use crate::types::{DeepstreamVersion, RecipeName, RunnerSize, TargetPlatform};

#[derive(Subcommand, Debug)]
pub enum Build {
    /// Build the vinca pipeline.
    Vinca {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: String,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: TargetPlatform,
        #[arg(long = "ds-recipe")]
        ds_recipes: Vec<RecipeName>,
        #[arg(long)]
        ds_version: Option<DeepstreamVersion>,
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
    },
    /// Build inside a DeepStream container.
    DeepstreamContainer {
        #[arg(long)]
        ds_version: DeepstreamVersion,
        #[arg(long)]
        target_platform: TargetPlatform,
    },
}

impl Build {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Vinca {
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
            } => vinca(
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                ds_recipes,
                ds_version,
            ),
            Self::Pixi {
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                runner_size,
            } => pixi(
                repo_root,
                channel_url,
                output_dir,
                target_platform,
                runner_size,
            ),
            Self::DeepstreamContainer { .. } => {
                anyhow::bail!("build deepstream-container: not implemented")
            }
        }
    }
}

use crate::process;
use crate::repo::Repo;

fn vinca(
    repo_root: Option<PathBuf>,
    channel_url: String,
    output_dir: PathBuf,
    target_platform: TargetPlatform,
    ds_recipes: Vec<RecipeName>,
    ds_version: Option<DeepstreamVersion>,
) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let mode = VincaBuildMode::from_flags(ds_recipes, ds_version)?;

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
    args.extend_from_slice(&[
        "-c",
        &channel_url,
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
        _ => None,
    }
}

use anyhow::Context;
use std::fs;
use std::path::Path;

/// Selects which subset of recipes to build and whether to pin a DeepStream version.
/// Maps to the three valid combinations of `--ds-recipe` and `--ds-version` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VincaBuildMode {
    /// No DS flags: build everything in `recipes/` across all DS variants.
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
}

impl VincaBuildMode {
    /// Construct from the parsed CLI flags. Rejects `--ds-version` without `--ds-recipe`,
    /// which would build everything against one pinned DS version — meaningless and
    /// almost certainly a CI misconfiguration.
    pub fn from_flags(
        recipes: Vec<RecipeName>,
        version: Option<DeepstreamVersion>,
    ) -> anyhow::Result<Self> {
        match (recipes.is_empty(), version) {
            (true, None) => Ok(Self::Normal),
            (false, None) => Ok(Self::DropDeepstream { recipes }),
            (false, Some(version)) => Ok(Self::DeepstreamOnly { recipes, version }),
            (true, Some(_)) => anyhow::bail!(
                "--ds-version requires at least one --ds-recipe (matrix compute always pairs them)"
            ),
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
        VincaBuildMode::DeepstreamOnly { recipes, .. } => {
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
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamPackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub build: Option<UpstreamBuild>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpstreamBuild {
    /// Defaults to 0 when omitted from the upstream pixi.toml.
    #[serde(default)]
    pub number: u64,
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
        self.package.build.as_ref().map(|b| b.number).unwrap_or(0)
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
}

use crate::types::{GitVersion, GithubRepoUrl, PixiNativeEntry};

/// Fetch `pixi.toml` for an entry from `raw.githubusercontent.com`. Uses Bearer
/// auth from `GITHUB_TOKEN` / `GH_TOKEN` when present.
fn fetch_pixi_toml(entry: &PixiNativeEntry) -> anyhow::Result<String> {
    let rev_or_ref = match &entry.version {
        GitVersion::Rev(sha) => sha.as_str().to_string(),
        GitVersion::Ref(r) => r.clone(),
    };

    let subdir = entry
        .subdir
        .as_deref()
        .map(|p| p.to_string_lossy().trim_matches('/').to_string())
        .filter(|s| !s.is_empty() && s != ".")
        .map(|s| format!("{s}/pixi.toml"))
        .unwrap_or_else(|| "pixi.toml".to_string());

    let raw_url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        entry.url.owner, entry.url.repo, rev_or_ref, subdir
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

/// Initialize a fresh git repo in `dest`, fetch one commit (`rev_or_ref`),
/// and check it out. `rev_or_ref` may be a 40-char SHA or a branch/tag name.
fn fetch_at_rev(url: &GithubRepoUrl, version: &GitVersion, dest: &Path) -> anyhow::Result<()> {
    let url_str = format!("https://github.com/{}/{}", url.owner, url.repo);
    let rev_or_ref = match version {
        GitVersion::Rev(sha) => sha.as_str().to_string(),
        GitVersion::Ref(r) => r.clone(),
    };

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
        &rev_or_ref,
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

fn pixi(
    repo_root: Option<PathBuf>,
    channel_url: String,
    output_dir: PathBuf,
    target_platform: TargetPlatform,
    runner_size: Option<RunnerSize>,
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

    for entry in &manifest.packages {
        if let Some(filter) = runner_size
            && entry.runner_size != filter
        {
            continue;
        }

        let pixi_toml_text = fetch_pixi_toml(entry)?;
        let upstream = UpstreamPixiToml::parse(&pixi_toml_text)
            .with_context(|| format!("entry {}: parse upstream pixi.toml", entry.name))?;

        if !upstream.supports_platform(target_platform) {
            tracing::info!(
                "skipping {} {}: pixi.toml does not list {}",
                upstream.package.name,
                upstream.package.version,
                target_platform.arch(),
            );
            continue;
        }

        if package_published(
            &upstream.package.name,
            &upstream.package.version,
            upstream.build_number(),
            &channel_url,
            target_platform,
        ) {
            tracing::info!(
                "skipping {} {}: already in channel {}",
                upstream.package.name,
                upstream.package.version,
                channel_url,
            );
            continue;
        }

        let tmp = tempfile::Builder::new()
            .prefix(&format!("pixi-native-{}-", entry.name))
            .tempdir()
            .context("create temp workdir")?;
        let workdir = tmp.path().join("src");
        fs::create_dir(&workdir)?;
        fetch_at_rev(&entry.url, &entry.version, &workdir)?;

        let subdir = entry.subdir.as_deref().unwrap_or(Path::new("."));
        let manifest_path = workdir.join(subdir).join("pixi.toml");
        if !manifest_path.is_file() {
            anyhow::bail!(
                "entry {}: no pixi.toml at {}/pixi.toml in checkout",
                entry.name,
                subdir.display(),
            );
        }

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
    }

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

    #[test]
    fn vinca_mode_normal_when_no_flags() {
        let m = VincaBuildMode::from_flags(vec![], None).unwrap();
        assert_eq!(m, VincaBuildMode::Normal);
    }

    #[test]
    fn vinca_mode_drop_when_only_recipes() {
        let m = VincaBuildMode::from_flags(vec![recipe("a"), recipe("b")], None).unwrap();
        assert_eq!(
            m,
            VincaBuildMode::DropDeepstream {
                recipes: vec![recipe("a"), recipe("b")]
            }
        );
    }

    #[test]
    fn vinca_mode_only_when_both_flags() {
        let m =
            VincaBuildMode::from_flags(vec![recipe("a")], Some(DeepstreamVersion::V7_1)).unwrap();
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
        let err = VincaBuildMode::from_flags(vec![], Some(DeepstreamVersion::V8_0)).unwrap_err();
        assert!(format!("{err:#}").contains("requires at least one --ds-recipe"));
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

[package.build]
number = 5
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
}
