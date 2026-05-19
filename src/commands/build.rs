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
