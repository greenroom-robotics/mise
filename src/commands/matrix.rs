use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Context;
use clap::Subcommand;
use serde::Serialize;

use crate::consts::{PIXI_NATIVE_PACKAGES_YAML, PIXI_TOML, ROSDISTRO_RECIPES_YAML};
use crate::gh::{self, ChangedFiles};
use crate::repo::{DeepstreamCfg, Repo};
use crate::types::{
    Arch, DeepstreamVersion, PackageName, PixiNativeEntry, PixiNativeManifest, RunnerSpec,
};

/// Vinca-only: these shape the vinca-generated `recipes/`, which the DS build
/// deletes (it keeps only `vendor_recipes/` DS recipes), so they cannot change
/// a DS build's output.
const GLOBAL_VINCA: &[&str] = &[
    "vinca.yaml",
    "robostack.yaml",
    "packages-ignore.yaml",
    "rosdistro_snapshot.yaml",
];

/// Vinca + `DeepStream`: variant pins feed both pipelines' build hashes.
const GLOBAL_VINCA_DS: &[&str] = &["conda_build_config.yaml"];

const DEEPSTREAM_ONLY: &[&str] = &["docker-compose.yml", "variants/deepstream.yaml"];

const GLOBAL_BOTH: &[&str] = &[PIXI_TOML, "pixi.lock"];

const GLOBAL_BOTH_PREFIXES: &[&str] = &[".github/workflows/", ".github/actions/", "scripts/"];

/// The two archs, each with its vinca-pipeline runner template.
const ARCHS: &[(Arch, &str)] = &[
    (Arch::Linux64, "runs-on={run_id}/runner=8cpu-linux-x64"),
    (
        Arch::LinuxAarch64,
        "runs-on={run_id}/runner=8cpu-linux-arm64",
    ),
];

const fn ds_runner_family(arch: Arch) -> &'static str {
    match arch {
        Arch::Linux64 => "c6id.xlarge",
        Arch::LinuxAarch64 => "c7gd.xlarge",
    }
}

/// m-family (4GB/cpu) override for himem buckets; the named runners default
/// to the c-family's 2GB/cpu.
const fn himem_family(arch: Arch) -> &'static str {
    match arch {
        Arch::Linux64 => "m7a+m7i",
        Arch::LinuxAarch64 => "m8g+m7g",
    }
}

const fn ds_arch_tag(arch: Arch) -> &'static str {
    match arch {
        Arch::Linux64 => "x64",
        Arch::LinuxAarch64 => "arm64",
    }
}

const fn ds_image_for(version: DeepstreamVersion) -> &'static str {
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
    let event = gh::Event::load()?;
    let changed = gh::changed_files(&repo, &event)?;

    let run_id = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "0".into());
    let plan = resolve(classify(&changed, &ds), &repo, &event)?;
    let entries = build_matrix(&plan, &manifest, &run_id);

    let has_work = !entries.is_empty();
    let entries = if has_work {
        entries
    } else {
        vec![MatrixRow::should_not_run(&run_id)]
    };

    let matrix_json = serde_json::to_string(&serde_json::json!({ "include": entries }))?;
    let recipes_csv = ds
        .recipes
        .iter()
        .map(super::super::types::RecipeName::as_str)
        .collect::<Vec<_>>()
        .join(",");

    let pixi_only = match &plan.pixi_native {
        PixiScope::Only(names) => names
            .iter()
            .map(PackageName::as_str)
            .collect::<Vec<_>>()
            .join(","),
        PixiScope::All | PixiScope::None => String::new(),
    };

    gh::outputs::set("matrix-json", &matrix_json)?;
    gh::outputs::set("recipes-csv", &recipes_csv)?;
    gh::outputs::set("has-work", &has_work)?;
    gh::outputs::set("pixi-only", &pixi_only)?;

    // Stdout copy for CI log visibility.
    println!("{matrix_json}");

    Ok(())
}

/// The `pipeline` field of a row — what the consuming workflow switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Pipeline {
    Vinca,
    PixiNative,
    /// Sentinel emitted when there is no work — trips a guard step in CI.
    ShouldNotRun,
}

/// What a matrix row is for, carrying exactly the fields that kind of row has.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKind {
    /// A plain vinca/rattler build of `recipes/`.
    Vinca,
    /// A pixi-native build job, which handles every entry of one runner spec.
    PixiNative { runner: RunnerSpec },
    /// A vinca build of the `DeepStream` recipes against one DS version.
    Deepstream { version: DeepstreamVersion },
    /// No work: one placeholder row, because an empty matrix is an error to
    /// GitHub Actions.
    ShouldNotRun,
}

impl RowKind {
    const fn pipeline(&self) -> Pipeline {
        match self {
            // A DeepStream row runs the vinca pipeline.
            Self::Vinca | Self::Deepstream { .. } => Pipeline::Vinca,
            Self::PixiNative { .. } => Pipeline::PixiNative,
            Self::ShouldNotRun => Pipeline::ShouldNotRun,
        }
    }
}

/// One row of the matrix JSON output.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MatrixRow {
    kind: RowKind,
    target_platform: Arch,
    runner: String,
    artifact_name: String,
}

impl MatrixRow {
    fn vinca(arch: Arch, runner_tmpl: &str, run_id: &str) -> Self {
        Self {
            kind: RowKind::Vinca,
            target_platform: arch,
            runner: runner_tmpl.replace("{run_id}", run_id),
            artifact_name: format!("build-{arch}"),
        }
    }

    fn pixi_native(arch: Arch, runner: RunnerSpec, run_id: &str) -> Self {
        let tag = ds_arch_tag(arch);
        let size = runner.size;
        let family = if runner.himem {
            format!("/family={}", himem_family(arch))
        } else {
            String::new()
        };
        Self {
            kind: RowKind::PixiNative { runner },
            target_platform: arch,
            runner: format!("runs-on={run_id}/runner={size}-linux-{tag}{family}"),
            artifact_name: format!("build-pixi-native-{arch}-{runner}"),
        }
    }

    fn deepstream(arch: Arch, version: DeepstreamVersion, run_id: &str) -> Self {
        let tag = ds_arch_tag(arch);
        let family = ds_runner_family(arch);
        Self {
            kind: RowKind::Deepstream { version },
            target_platform: arch,
            runner: format!("runs-on={run_id}/family={family}/image=deepstream-{tag}-{version}"),
            artifact_name: format!("build-deepstream-{arch}-ds{version}"),
        }
    }

    fn should_not_run(run_id: &str) -> Self {
        Self {
            kind: RowKind::ShouldNotRun,
            target_platform: Arch::Linux64,
            runner: format!("runs-on={run_id}/runner=1cpu-linux-x64"),
            artifact_name: "should-not-run".into(),
        }
    }
}

/// Every row carries every key, with `""` where the kind has no such field —
/// the shape the consuming workflow YAML expects.
impl Serialize for MatrixRow {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (ds_version, ds_image) = match &self.kind {
            RowKind::Deepstream { version } => (version.to_string(), ds_image_for(*version)),
            _ => (String::new(), ""),
        };
        let runner_size = match &self.kind {
            RowKind::PixiNative { runner } => runner.to_string(),
            _ => String::new(),
        };
        let mut row = s.serialize_struct("MatrixRow", 7)?;
        row.serialize_field("pipeline", &self.kind.pipeline())?;
        row.serialize_field("target-platform", &self.target_platform)?;
        row.serialize_field("ds-version", &ds_version)?;
        row.serialize_field("ds-image", ds_image)?;
        row.serialize_field("runner", &self.runner)?;
        row.serialize_field("runner-size", &runner_size)?;
        row.serialize_field("artifact-name", &self.artifact_name)?;
        row.end()
    }
}

/// Pixi-native scope as far as the changed-file list alone can say;
/// [`resolve`] turns `ManifestScoped` into concrete package names.
#[derive(Debug, Default, PartialEq, Eq)]
enum RawScope {
    /// No pixi-native work.
    #[default]
    None,
    /// Build every package (dispatch, or a global file changed).
    All,
    /// `pixi_native_packages.yaml` changed; which entries changed is not known
    /// until the manifest is diffed against the base ref.
    ManifestScoped,
}

/// Which pixi-native packages to build — the resolved form [`build_matrix`]
/// accepts.
#[derive(Debug, Default, PartialEq, Eq)]
enum PixiScope {
    /// No pixi-native work.
    #[default]
    None,
    /// Build every package.
    All,
    /// Build only the named packages. Non-empty: an empty change set resolves
    /// to `None` instead.
    Only(BTreeSet<PackageName>),
}

/// What the changed-file list alone says about this run.
#[derive(Debug, Default, PartialEq, Eq)]
struct RawState {
    vinca: bool,
    pixi_native: RawScope,
    ds_versions: BTreeSet<DeepstreamVersion>,
}

/// A [`RawState`] with its pixi-native scope resolved.
#[derive(Debug, Default, PartialEq, Eq)]
struct MatrixPlan {
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
) -> anyhow::Result<BTreeSet<PackageName>> {
    let head = PixiNativeManifest::from_yaml_str(head_yaml)?;
    let Some(base_yaml) = base_yaml else {
        return Ok(head.packages.iter().map(|e| e.name.clone()).collect());
    };
    let base = PixiNativeManifest::from_yaml_str(base_yaml)?;
    let base_by_name: BTreeMap<&PackageName, &PixiNativeEntry> =
        base.packages.iter().map(|e| (&e.name, e)).collect();

    let mut changed = BTreeSet::new();
    for e in &head.packages {
        match base_by_name.get(&e.name) {
            None => {
                changed.insert(e.name.clone());
            }
            Some(b) => {
                if b.url != e.url
                    || b.rev.as_str() != e.rev.as_str()
                    || b.subdir != e.subdir
                    || b.runner_spec() != e.runner_spec()
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

fn resolve(raw: RawState, repo: &Repo, event: &gh::Event) -> anyhow::Result<MatrixPlan> {
    let pixi_native = match raw.pixi_native {
        RawScope::None => PixiScope::None,
        RawScope::All => PixiScope::All,
        RawScope::ManifestScoped => resolve_pixi_scope(repo, event)?,
    };
    Ok(MatrixPlan {
        vinca: raw.vinca,
        pixi_native,
        ds_versions: raw.ds_versions,
    })
}

fn classify(changed: &ChangedFiles, ds: &DeepstreamCfg) -> RawState {
    let mut state = RawState::default();

    let paths: &[std::path::PathBuf] = match changed {
        ChangedFiles::All => {
            state.vinca = true;
            state.pixi_native = RawScope::All;
            state.ds_versions = ds.versions.clone();
            return state;
        }
        ChangedFiles::Paths(p) => p,
    };

    for path in paths {
        let Some(p) = path.to_str() else { continue };

        if GLOBAL_VINCA.contains(&p) {
            state.vinca = true;
            continue;
        }
        if GLOBAL_VINCA_DS.contains(&p) {
            state.vinca = true;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if DEEPSTREAM_ONLY.contains(&p) {
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if GLOBAL_BOTH.contains(&p)
            || GLOBAL_BOTH_PREFIXES
                .iter()
                .any(|prefix| p.starts_with(prefix))
        {
            state.vinca = true;
            state.pixi_native = RawScope::All;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if p == ".github/deepstream-recipes.yaml" {
            state.vinca = true;
            state.ds_versions.extend(ds.versions.iter().copied());
            continue;
        }
        if p == ROSDISTRO_RECIPES_YAML {
            state.vinca = true;
            continue;
        }
        if p == PIXI_NATIVE_PACKAGES_YAML {
            if state.pixi_native != RawScope::All {
                state.pixi_native = RawScope::ManifestScoped;
            }
            continue;
        }
        if let Some(rest) = p.strip_prefix("vendor_recipes/") {
            let name = rest.split('/').next().unwrap_or("");
            if ds.recipes.iter().any(|r| r.as_str() == name) {
                state.ds_versions.extend(ds.versions.iter().copied());
            } else {
                state.vinca = true;
            }
            continue;
        }
        // `recipes/` is vinca-generated; changes there trigger nothing.
        if p.starts_with("recipes/") {
            continue;
        }
    }

    state
}

fn build_matrix(plan: &MatrixPlan, manifest: &PixiNativeManifest, run_id: &str) -> Vec<MatrixRow> {
    let mut out = Vec::new();

    if plan.vinca {
        for (arch, runner_tmpl) in ARCHS {
            out.push(MatrixRow::vinca(*arch, runner_tmpl, run_id));
        }
    }

    // One job per distinct runner spec; each job builds every entry of its spec.
    let pixi_sizes: BTreeSet<RunnerSpec> = match &plan.pixi_native {
        PixiScope::All => manifest
            .packages
            .iter()
            .map(super::super::types::PixiNativeEntry::runner_spec)
            .collect(),
        PixiScope::Only(names) => manifest
            .packages
            .iter()
            .filter(|e| names.contains(&e.name))
            .map(super::super::types::PixiNativeEntry::runner_spec)
            .collect(),
        PixiScope::None => BTreeSet::new(),
    };
    for size in pixi_sizes {
        for (arch, _) in ARCHS {
            out.push(MatrixRow::pixi_native(*arch, size, run_id));
        }
    }

    for ver in &plan.ds_versions {
        for (arch, _) in ARCHS {
            out.push(MatrixRow::deepstream(*arch, *ver, run_id));
        }
    }

    out
}

#[cfg(test)]
#[path = "matrix_tests.rs"]
mod tests;
