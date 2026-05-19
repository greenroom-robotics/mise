use clap::{Parser, Subcommand};

use crate::commands::{build::Build, bump::Bump, matrix::Matrix, snapshot::Snapshot};

#[derive(Parser, Debug)]
#[command(name = "mise", about = "Build/bump/matrix automation for ros-recipes")]
pub struct Cli {
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    /// Build-matrix computation.
    #[command(subcommand)]
    Matrix(Matrix),
    /// Package builds.
    #[command(subcommand)]
    Build(Build),
    /// Version bumps.
    #[command(subcommand)]
    Bump(Bump),
    /// Snapshot maintenance.
    #[command(subcommand)]
    Snapshot(Snapshot),
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        match self.command {
            Top::Matrix(c) => c.run(),
            Top::Build(c) => c.run(),
            Top::Bump(c) => c.run(),
            Top::Snapshot(c) => c.run(),
        }
    }
}
