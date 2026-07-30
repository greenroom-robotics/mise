use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Context;
use clap::Subcommand;
use serde::Serialize;

use crate::consts::{PIXI_NATIVE_PACKAGES_YAML, PIXI_TOML, ROSDISTRO_RECIPES_YAML};
use crate::gh::{self, ChangedFiles};
use crate::repo::{DeepstreamCfg, Repo};
use crate::types::{Arch, DeepstreamVersion, PixiNativeEntry, PixiNativeManifest, RunnerSize};

const GLOBAL_VINCA: &[&str] = &[
    "vinca.yaml",
    "conda_build_config.yaml",
    "robostack.yaml",
    "packages-ignore.yaml",
    "rosdistro_snapshot.yaml",
];

const GLOBAL_BOTH: &[&str] = &[PIXI_TOML, "pixi.lock"];

const GLOBAL_BOTH_PREFIXES: &[&str] = &[".github/workflows/", ".github/actions/", "scripts/"];

/// The two archs and their default runner template (used by the vinca pipeline).
const ARCHS: &[(Arch, &str)] = &[
    (Arch::Linux64, "runs-on={run_id}/runner=8cpu-linux-x64"),
    (
        Arch::LinuxAarch64,
        "runs-on={run_id}/runner=8cpu-linux-arm64",
    ),
];

fn ds_runner_family(arch: Arch) -> &'static str {
    match arch {
        Arch::Linux64 => "c6id.xlarge",
        Arch::LinuxAarch64 => "c7gd.xlarge",
    }
}

fn ds_arch_tag(arch: Arch) -> &'static str {
    match arch {
        Arch::Linux64 => "x64",
        Arch::LinuxAarch64 => "arm64",
    }
}

fn ds_image_for(version: DeepstreamVersion) -> &'static str {
    match version {
        DeepstreamVersion::V7_1 => "nvcr.io/nvidia/deepstream:7.1-triton-multiarch",
        DeepstreamVersion::V8_0 => "nvcr.io/nvidia/deepstream:8.0-triton-multiarch",
    }
}

#[derive(Subcommand, Debug)]
pub enum Matrix {
    /// Compute the build matrix for CI.
    Compute {
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
}

impl Matrix {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Compute { repo_root } => compute(repo_root),
        }
    }
}

fn compute(repo_root: Option<PathBuf>) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let ds = repo.deepstream()?;
    let manifest = repo.pixi_native_manifest()?;
    let event = gh::rebase_push_to_last_publish(gh::Event::load()?)?;
    let changed = gh::changed_files(&repo, &event)?;

    let run_id = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "0".into());
    let mut state = classify(&changed, &ds);
    if state.pixi_native == PixiScope::ManifestScoped {
        state.pixi_native = resolve_pixi_scope(&repo, &event)?;
    }
    let entries = build_matrix(&state, &manifest, &run_id);

    let has_work = !entries.is_empty();
    let entries = if has_work {
        entries
    } else {
        vec![placeholder_entry(&run_id)]
    };

    let matrix_json = serde_json::to_string(&serde_json::json!({ "include": entries }))?;
    let recipes_csv = ds
        .recipes
        .iter()
        .map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let pixi_only = match &state.pixi_native {
        PixiScope::Only(names) => names.iter().cloned().collect::<Vec<_>>().join(","),
        _ => String::new(),
    };

    gh::outputs::set("matrix-json", &matrix_json)?;
    gh::outputs::set("recipes-csv", &recipes_csv)?;
    gh::outputs::set("has-work", &has_work)?;
    gh::outputs::set("pixi-only", &pixi_only)?;

    // Always print matrix_json to stdout — matches the Python script for log visibility.
    println!("{matrix_json}");

    Ok(())
}

fn placeholder_entry(run_id: &str) -> MatrixEntry {
    MatrixEntry {
        pipeline: Pipeline::ShouldNotRun,
        target_platform: Arch::Linux64,
        ds_version: String::new(),
        ds_image: String::new(),
        runner: format!("runs-on={run_id}/runner=1cpu-linux-x64"),
        runner_size: String::new(),
        artifact_name: "should-not-run".into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pipeline {
    Vinca,
    PixiNative,
    /// Sentinel emitted when there is no work — trips a guard step in CI.
    ShouldNotRun,
}

/// One row of the matrix JSON output. Field names mirror the original Python
/// (kebab-case) so the consuming workflow YAML keeps working unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatrixEntry {
    pub pipeline: Pipeline,
    #[serde(rename = "target-platform")]
    pub target_platform: Arch,
    /// Empty string for non-DS rows (matches Python output exactly).
    #[serde(rename = "ds-version")]
    pub ds_version: String,
    #[serde(rename = "ds-image")]
    pub ds_image: String,
    pub runner: String,
    /// Empty for vinca and DS rows; one of "4cpu", "8cpu", "16cpu", "32cpu" for pixi-native rows.
    #[serde(rename = "runner-size")]
    pub runner_size: String,
    #[serde(rename = "artifact-name")]
    pub artifact_name: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum PixiScope {
    /// No pixi-native work.
    #[default]
    None,
    /// Build every package (dispatch, or a global file changed).
    All,
    /// `pixi_native_packages.yaml` changed; specific names resolved by `compute()`.
    ManifestScoped,
    /// Build only the named packages.
    Only(BTreeSet<String>),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MatrixState {
    vinca: bool,
    pixi_native: PixiScope,
    ds_versions: BTreeSet<DeepstreamVersion>,
}

/// Names of packages added or whose url/rev/subdir/runner-size changed between
/// `base_yaml` (None when the manifest did not exist at the base ref) and
/// `head_yaml`. Removed packages are ignored (nothing to build).
fn diff_changed_packages(
    base_yaml: Option<&str>,
    head_yaml: &str,
) -> anyhow::Result<BTreeSet<String>> {
    let head = PixiNativeManifest::from_yaml_str(head_yaml)?;
    let Some(base_yaml) = base_yaml else {
        return Ok(head.packages.iter().map(|e| e.name.clone()).collect());
    };
    let base = PixiNativeManifest::from_yaml_str(base_yaml)?;
    let base_by_name: BTreeMap<&str, &PixiNativeEntry> =
        base.packages.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut changed = BTreeSet::new();
    for e in &head.packages {
        match base_by_name.get(e.name.as_str()) {
            None => {
                changed.insert(e.name.clone());
            }
            Some(b) => {
                if b.url != e.url
                    || b.rev.as_str() != e.rev.as_str()
                    || b.subdir != e.subdir
                    || b.runner_size != e.runner_size
                {
                    changed.insert(e.name.clone());
                }
            }
        }
    }
    Ok(changed)
}

/// Resolve a `ManifestScoped` state into the concrete set of changed package
/// names by diffing `pixi_native_packages.yaml` against the base ref.
fn resolve_pixi_scope(repo: &Repo, event: &gh::Event) -> anyhow::Result<PixiScope> {
    let Some(base) = event.base_sha() else {
        // No base ref to diff against — fail safe by building everything.
        return Ok(PixiScope::All);
    };
    let head_yaml = std::fs::read_to_string(repo.root().join(PIXI_NATIVE_PACKAGES_YAML))
        .with_context(|| format!("read {PIXI_NATIVE_PACKAGES_YAML}"))?;
    let base_yaml = crate::git::file_at_rev(repo.root(), base, PIXI_NATIVE_PACKAGES_YAML)?;
    let changed = diff_changed_packages(base_yaml.as_deref(), &head_yaml)?;
    if changed.is_empty() {
        Ok(PixiScope::None)
    } else {
        Ok(PixiScope::Only(changed))
    }
}

fn classify(changed: &ChangedFiles, ds: &DeepstreamCfg) -> MatrixState {
    let mut state = MatrixState::default();

    let paths: &[std::path::PathBuf] = match changed {
        ChangedFiles::All => {
            state.vinca = true;
            state.pixi_native = PixiScope::All;
            state.ds_versions = ds.versions.clone();
            return state;
        }
        ChangedFiles::Paths(p) => p,
    };

    for path in paths {
        let Some(p) = path.to_str() else { continue };

        if GLOBAL_VINCA.contains(&p) {
            state.vinca = true;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if GLOBAL_BOTH.contains(&p)
            || GLOBAL_BOTH_PREFIXES
                .iter()
                .any(|prefix| p.starts_with(prefix))
        {
            state.vinca = true;
            state.pixi_native = PixiScope::All;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if p == ".github/deepstream-recipes.yaml" {
            state.vinca = true;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if p == "variants/deepstream.yaml" {
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if p == ROSDISTRO_RECIPES_YAML {
            state.vinca = true;
            continue;
        }
        if p == PIXI_NATIVE_PACKAGES_YAML {
            if state.pixi_native != PixiScope::All {
                state.pixi_native = PixiScope::ManifestScoped;
            }
            continue;
        }
        if let Some(rest) = p.strip_prefix("vendor_recipes/") {
            let name = rest.split('/').next().unwrap_or("");
            // RecipeName has no validation yet, so use the string directly.
            if ds.recipes.iter().any(|r| r.as_str() == name) {
                state.ds_versions.extend(ds.versions.iter().copied());
            } else {
                state.vinca = true;
            }
            continue;
        }
        // `recipes/` is generated by vinca; ignore. Everything else (docs, README) → no jobs.
        if p.starts_with("recipes/") {
            continue;
        }
    }

    state
}

fn build_matrix(
    state: &MatrixState,
    manifest: &PixiNativeManifest,
    run_id: &str,
) -> Vec<MatrixEntry> {
    let mut out = Vec::new();

    if state.vinca {
        for (arch, runner_tmpl) in ARCHS {
            out.push(MatrixEntry {
                pipeline: Pipeline::Vinca,
                target_platform: *arch,
                ds_version: String::new(),
                ds_image: String::new(),
                runner: runner_tmpl.replace("{run_id}", run_id),
                runner_size: String::new(),
                artifact_name: format!("build-{arch}"),
            });
        }
    }

    let pixi_sizes: BTreeSet<RunnerSize> = match &state.pixi_native {
        PixiScope::All => manifest.packages.iter().map(|e| e.runner_size).collect(),
        PixiScope::Only(names) => manifest
            .packages
            .iter()
            .filter(|e| names.contains(&e.name))
            .map(|e| e.runner_size)
            .collect(),
        PixiScope::None | PixiScope::ManifestScoped => BTreeSet::new(),
    };
    for size in pixi_sizes {
        let size_str = size.to_string();
        for (arch, _) in ARCHS {
            let tag = ds_arch_tag(*arch);
            out.push(MatrixEntry {
                pipeline: Pipeline::PixiNative,
                target_platform: *arch,
                ds_version: String::new(),
                ds_image: String::new(),
                runner: format!("runs-on={run_id}/runner={size_str}-linux-{tag}"),
                runner_size: size_str.clone(),
                artifact_name: format!("build-pixi-native-{arch}-{size_str}"),
            });
        }
    }

    for ver in &state.ds_versions {
        for (arch, _) in ARCHS {
            let tag = ds_arch_tag(*arch);
            let family = ds_runner_family(*arch);
            out.push(MatrixEntry {
                pipeline: Pipeline::Vinca,
                target_platform: *arch,
                ds_version: ver.to_string(),
                ds_image: ds_image_for(*ver).to_string(),
                runner: format!("runs-on={run_id}/family={family}/image=deepstream-{tag}-{ver}"),
                runner_size: String::new(),
                artifact_name: format!("build-deepstream-{arch}-ds{ver}"),
            });
        }
    }

    out
}

#[cfg(test)]
#[path = "matrix_tests.rs"]
mod tests;
