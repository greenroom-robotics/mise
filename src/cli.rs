use clap::{Parser, Subcommand};

use crate::commands::{build_recipes::BuildRecipes, ci::Ci, matrix::Matrix, snapshot::Snapshot};

#[derive(Parser, Debug)]
#[command(name = "mise", about = "Build/matrix automation for ros-recipes")]
pub struct Cli {
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    /// Build-matrix computation.
    #[command(subcommand)]
    Matrix(Matrix),
    /// Recipe builds (vinca, pixi-native, DeepStream container).
    #[command(subcommand)]
    BuildRecipes(BuildRecipes),
    /// CI helpers for pixi-native ROS package repos.
    #[command(subcommand)]
    Ci(Ci),
    /// Snapshot maintenance.
    #[command(subcommand)]
    Snapshot(Snapshot),
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            Top::Matrix(c) => c.run(),
            Top::BuildRecipes(c) => c.run(),
            Top::Ci(c) => c.run(),
            Top::Snapshot(c) => c.run(),
        }
    }
}
