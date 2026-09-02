use clap::Subcommand;

pub mod build;
pub mod bump_pixi;
pub mod recipes_pr;
pub mod recipes_upsert;
pub mod release;
pub mod siblings;
pub mod sync_cargo;
pub mod test;
pub mod verify_siblings;
mod yaml_block;

use build::Build;
use bump_pixi::BumpPixi;
use recipes_pr::RecipesPr;
use release::Release;
use sync_cargo::SyncCargo;
use test::Test;
use verify_siblings::VerifySiblings;

#[derive(Subcommand, Debug)]
pub enum Ci {
    /// Run tests for one or more pixi-native ROS packages.
    Test(Test),
    /// Build one or more pixi-native ROS packages to .conda artifacts.
    Build(Build),
    /// Run semantic-release for one or more pixi-native ROS packages.
    Release(Release),
    /// Callback invoked by semantic-release prepare hook to write the new version into pixi.toml. Not for direct use.
    BumpPixi(BumpPixi),
    /// Callback invoked by semantic-release publish hook to open the recipes-repo PR. Not for direct use.
    RecipesPr(RecipesPr),
    /// mise-specific prepare callback to sync Cargo.toml/Cargo.lock to the released version. Not for direct use.
    SyncCargo(SyncCargo),
    /// Callback invoked by semantic-release prepare hook to check sibling path deps against their release tags. Not for direct use.
    VerifySiblings(VerifySiblings),
}

impl Ci {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        match self {
            Self::Test(c) => c.run(),
            Self::Build(c) => c.run(),
            Self::Release(c) => c.run(),
            Self::BumpPixi(c) => c.run(),
            Self::RecipesPr(c) => c.run(),
            Self::SyncCargo(c) => c.run(),
            Self::VerifySiblings(c) => c.run(),
        }
    }
}
