use std::path::PathBuf;

use clap::Subcommand;

use crate::types::{DeepstreamVersion, RecipeName, TargetPlatform};

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
            Self::Vinca { .. } => anyhow::bail!("build vinca: not implemented"),
            Self::Pixi { .. } => anyhow::bail!("build pixi: not implemented"),
            Self::DeepstreamContainer { .. } => {
                anyhow::bail!("build deepstream-container: not implemented")
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

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
}
