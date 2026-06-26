use clap::Subcommand;

pub mod build;
pub mod bump_pixi;
pub mod lock_check;
pub mod packages;
pub mod pixi_meta;
pub mod recipes_pr;
pub mod recipes_upsert;
pub mod release;
pub mod test;

use build::Build;
use bump_pixi::BumpPixi;
use lock_check::LockCheck;
use recipes_pr::RecipesPr;
use release::Release;
use test::Test;

#[derive(Subcommand, Debug)]
pub enum Ci {
    /// Run tests for one or more pixi-native ROS packages.
    Test(Test),
    /// Build one or more pixi-native ROS packages to .conda artifacts.
    Build(Build),
    /// Verify each package's pixi.lock is present and in sync with its pixi.toml.
    LockCheck(LockCheck),
    /// Run semantic-release for one or more pixi-native ROS packages.
    Release(Release),
    /// Callback invoked by semantic-release prepare hook to write the new version into pixi.toml. Not for direct use.
    BumpPixi(BumpPixi),
    /// Callback invoked by semantic-release publish hook to open the recipes-repo PR. Not for direct use.
    RecipesPr(RecipesPr),
}

impl Ci {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Ci::Test(c) => c.run(),
            Ci::Build(c) => c.run(),
            Ci::LockCheck(c) => c.run(),
            Ci::Release(c) => c.run(),
            Ci::BumpPixi(c) => c.run(),
            Ci::RecipesPr(c) => c.run(),
        }
    }
}
