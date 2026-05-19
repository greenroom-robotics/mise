use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Matrix {
    /// Compute the build matrix for CI.
    Compute {
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
}

impl Matrix {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Compute { .. } => anyhow::bail!("matrix compute: not implemented"),
        }
    }
}
