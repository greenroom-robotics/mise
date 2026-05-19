use std::collections::BTreeSet;
use std::path::PathBuf;

use clap::Subcommand;
use serde::Serialize;

use crate::types::{Arch, DeepstreamVersion};

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
            Self::Compute { repo_root: _ } => anyhow::bail!("matrix compute: not implemented"),
        }
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
#[allow(dead_code)]
struct MatrixState {
    vinca: bool,
    pixi_native: bool,
    ds_versions: BTreeSet<DeepstreamVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_serializes_to_kebab_case() {
        assert_eq!(serde_json::to_string(&Pipeline::Vinca).unwrap(), "\"vinca\"");
        assert_eq!(serde_json::to_string(&Pipeline::PixiNative).unwrap(), "\"pixi-native\"");
        assert_eq!(serde_json::to_string(&Pipeline::ShouldNotRun).unwrap(), "\"should-not-run\"");
    }

    #[test]
    fn matrix_entry_serializes_with_kebab_case_keys() {
        let e = MatrixEntry {
            pipeline: Pipeline::Vinca,
            target_platform: Arch::Linux64,
            ds_version: String::new(),
            ds_image: String::new(),
            runner: "runs-on=1/runner=4cpu-linux-x64".into(),
            runner_size: String::new(),
            artifact_name: "build-linux-64".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"target-platform\":\"linux-64\""), "got: {json}");
        assert!(json.contains("\"ds-version\":\"\""), "got: {json}");
        assert!(json.contains("\"runner-size\":\"\""), "got: {json}");
        assert!(json.contains("\"artifact-name\":\"build-linux-64\""), "got: {json}");
    }
}
