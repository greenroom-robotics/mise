//! `mise build-recipes vinca` — generate `recipes/` from `vinca.yaml`, filter
//! it down to the requested subset, and hand it to rattler-build.
//!
//! Also holds `deepstream-container`, which is that same pipeline preceded by
//! the prep a `DeepStream` container needs before it can run one.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::WrapErr;
use tempfile::NamedTempFile;

use crate::consts::ROBOSTACK_CHANNEL;
use crate::gh;
use crate::process;
use crate::repo::Repo;
use crate::types::{Arch, ChannelUrl, DeepstreamVersion, RecipeName};

#[allow(clippy::too_many_arguments)]
pub(super) fn vinca(
    repo_root: Option<PathBuf>,
    channel_url: ChannelUrl,
    overrides_channel_url: Option<ChannelUrl>,
    output_dir: PathBuf,
    target_platform: Arch,
    ds_recipes: Vec<RecipeName>,
    ds_version: Option<DeepstreamVersion>,
    only: Vec<RecipeName>,
) -> color_eyre::eyre::Result<()> {
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

    let arch_str = target_platform.to_string();

    process::run_in(
        repo.root(),
        "pixi",
        &["run", "vinca", "-m", "--platform", &arch_str],
    )?;

    apply_recipe_filter(repo.root(), &mode)?;

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
    let channel_url = channel_url.to_string();
    let overrides_channel_url = overrides_channel_url.map(|c| c.to_string());
    args.extend_from_slice(&["-c", &channel_url]);
    // The overrides channel lets --skip-existing find already-published
    // override packages; its packages carry down_prioritize_variant so the
    // solver still prefers the stock build for any dependency.
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

/// Selects which subset of recipes to build and whether to pin a `DeepStream` version.
/// Maps to the valid combinations of `--ds-recipe`, `--ds-version`, and `--only` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VincaBuildMode {
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
    fn from_flags(
        recipes: Vec<RecipeName>,
        version: Option<DeepstreamVersion>,
        only: Vec<RecipeName>,
    ) -> color_eyre::eyre::Result<Self> {
        if !only.is_empty() && !recipes.is_empty() {
            color_eyre::eyre::bail!("--only and --ds-recipe are mutually exclusive");
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
                color_eyre::eyre::bail!(
                    "--ds-version requires at least one --ds-recipe or --only recipe"
                )
            }
        }
    }

    /// The `DeepStream` version this mode pins the variant axis to, if any.
    const fn version(&self) -> Option<DeepstreamVersion> {
        match self {
            Self::DeepstreamOnly { version, .. } => Some(*version),
            Self::Only { version, .. } => *version,
            _ => None,
        }
    }
}

/// Prepare `<repo>/recipes` for rattler-build: overlay `vendor_recipes/`
/// entries (handrolled; they win over the stale vinca-generated versions),
/// remove `recipes/deepstream-mutex` (a payload-less noarch metapackage
/// consumed from the channel, never built here), then apply the mode's
/// filter.
fn apply_recipe_filter(repo_root: &Path, mode: &VincaBuildMode) -> color_eyre::eyre::Result<()> {
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
            let keep: std::collections::HashSet<&str> = recipes
                .iter()
                .map(crate::types::RecipeName::as_str)
                .collect();
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

fn copy_dir_all(src: &Path, dst: &Path) -> color_eyre::eyre::Result<()> {
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

/// Write a one-off variants YAML pinning the DS axis (and, for DS 7.1, the gcc
/// version that nvcc accepts). The caller must keep the returned
/// `NamedTempFile` alive until rattler-build has read the path.
fn write_variants_pin(version: DeepstreamVersion) -> color_eyre::eyre::Result<NamedTempFile> {
    // rattler-build's `-m` flag takes file paths, not KEY=VALUE.
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

pub(super) fn deepstream_container(
    repo_root: Option<PathBuf>,
    channel_url: ChannelUrl,
    output_dir: PathBuf,
    target_platform: Arch,
    ds_recipes: Vec<RecipeName>,
    ds_version: DeepstreamVersion,
) -> color_eyre::eyre::Result<()> {
    let repo = Repo::or_discover(repo_root)?;

    // The container cannot inherit the runner's git config, so auth for
    // private repo clones has to be configured here.
    gh::ensure_git_auth()?;

    // Host's .pixi/ has host-absolute shebangs that fail in-container; rebuild.
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
            let _ = fs::remove_dir_all(p);
        }
    }

    process::run_in(repo.root(), "pixi", &["install"])?;

    // DeepStream builds never touch overrides packages, so no overrides
    // channel is passed.
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
#[path = "vinca_tests.rs"]
mod tests;
