use clap::Subcommand;

pub mod build;
pub mod packages;
pub mod test;

use build::Build;
use test::Test;

#[derive(Subcommand, Debug)]
pub enum Ci {
    /// Run tests for one or more pixi-native ROS packages.
    Test(Test),
    /// Build one or more pixi-native ROS packages to .conda artifacts.
    Build(Build),
}

impl Ci {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Ci::Test(c) => c.run(),
            Ci::Build(c) => c.run(),
        }
    }
}
