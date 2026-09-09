use clap::{Parser, Subcommand};

use crate::commands::{ci::Ci, route::Route, snapshot::Snapshot};

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
    /// Snapshot maintenance.
    #[command(subcommand)]
    Snapshot(Snapshot),
    /// Package routing
    Route(Route),
}

impl Cli {
    /// Run the mise cli
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        match self.command {
            Top::Ci(c) => c.run(),
            Top::Snapshot(c) => c.run(),
            Top::Route(c) => c.run(),
        }
    }
}
