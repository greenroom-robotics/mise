//! `mise build-recipes` — the two build pipelines this repo drives.
//!
//! This module holds only the subcommand enum and its dispatch; each pipeline
//! lives in its own module: [`vinca`] (recipe generation + rattler-build, and
//! the container wrapper around it) and [`pixi`] (the pixi-native publish
//! check and build loop, supported by [`channel`], [`plan`] and
//! [`local_deps`]).

use std::path::PathBuf;

use clap::Subcommand;

use crate::types::{
    Arch, ChannelUrl, DeepstreamVersion, PackageName, RecipeName, RemoteChannel, RunnerSpec,
};

mod channel;
mod local_deps;
mod pixi;
mod plan;
mod vinca;

#[derive(Subcommand, Debug)]
pub enum BuildRecipes {
    /// Build the vinca pipeline.
    Vinca {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: ChannelUrl,
        /// Extra channel whose already-published packages should be skipped
        /// (rattler `--skip-existing`) but which must NOT win dependency
        /// resolution — the `overrides` channel. Its packages carry
        /// `down_prioritize_variant`, so the solver avoids them for build deps
        /// while `--skip-existing` still finds them to skip a rebuild.
        #[arg(long)]
        overrides_channel_url: Option<ChannelUrl>,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: Arch,
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
        /// The channel the publish check is answered from. Remote by
        /// construction: it is swept once and cached for the whole job, which
        /// is only sound for a channel this job cannot itself mutate.
        #[arg(long)]
        channel_url: RemoteChannel,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: Arch,
        /// Optional filter: only build entries in this runner bucket, e.g.
        /// `16cpu` or `16cpu-himem`.
        #[arg(long)]
        runner_size: Option<RunnerSpec>,
        /// Build only the listed package(s) by name. Empty = build all.
        #[arg(long = "only")]
        only: Vec<PackageName>,
    },
    /// Run a vinca build inside a DeepStream container. Does container-side prep
    /// (git auth, cache cleanup, `pixi install`) and delegates to `build vinca`
    /// with `--ds-version` and the recipe list pinned.
    DeepstreamContainer {
        #[arg(long)]
        repo_root: Option<PathBuf>,
        #[arg(long)]
        channel_url: ChannelUrl,
        #[arg(long, default_value = "./conda-bld")]
        output_dir: PathBuf,
        #[arg(long, default_value = "linux-64")]
        target_platform: Arch,
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
            } => vinca::vinca(
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
            } => pixi::pixi(
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
            } => vinca::deepstream_container(
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
