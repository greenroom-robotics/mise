use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Snapshot {
    /// Refresh the rosdistro snapshot.
    Refresh,
}

impl Snapshot {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Refresh => anyhow::bail!("snapshot refresh: not implemented"),
        }
    }
}
