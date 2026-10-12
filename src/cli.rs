use clap::{Parser, Subcommand};

use crate::commands::ci::Ci;

#[derive(Parser, Debug)]
#[command(
    name = "mise",
    version,
    about = "Build automation for a conda recipes repository"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    /// CI helpers for pixi-native ROS package repos.
    #[command(subcommand)]
    Ci(Ci),
}

impl Cli {
    /// Run the mise cli
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        match self.command {
            Top::Ci(c) => c.run(),
        }
    }
}
