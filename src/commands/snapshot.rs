use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Snapshot {
    /// Refresh the rosdistro snapshot.
    Refresh {
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
}

impl Snapshot {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Refresh { .. } => anyhow::bail!("snapshot refresh: not implemented"),
        }
    }
}
