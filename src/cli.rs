use clap::{Parser, Subcommand};

use crate::commands::{
    build_recipes::BuildRecipes, ci::Ci, matrix::Matrix, route::Route, snapshot::Snapshot,
};

#[derive(Parser, Debug)]
#[command(
    name = "mise",
    version,
    about = "Build/matrix automation for a conda recipes repository"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    /// Build-matrix computation.
    #[command(subcommand)]
    Matrix(Matrix),
    /// Recipe builds (vinca, pixi-native, `DeepStream` container).
    #[command(subcommand)]
    BuildRecipes(BuildRecipes),
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
            Top::Matrix(c) => c.run(),
            Top::BuildRecipes(c) => c.run(),
            Top::Ci(c) => c.run(),
            Top::Snapshot(c) => c.run(),
            Top::Route(c) => c.run(),
        }
    }
}
