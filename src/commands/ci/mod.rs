use clap::Subcommand;

pub mod test;

use test::Test;

#[derive(Subcommand, Debug)]
pub enum Ci {
    /// Run tests for one or more pixi-native ROS packages.
    Test(Test),
}

impl Ci {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Ci::Test(c) => c.run(),
        }
    }
}
