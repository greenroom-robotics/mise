use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Bump {
    /// Bump a recipe entry's version.
    Recipe { recipe: String, version: String },
    /// Bump a pixi-native package entry's rev or ref.
    Pixi { name: String, rev_or_ref: String },
    /// Route a dispatch payload to the appropriate bump subcommand.
    Route {
        #[arg(long)]
        payload: PathBuf,
    },
    /// Open a PR for an applied bump.
    OpenPr {
        #[arg(long)]
        bump_result: PathBuf,
    },
}

impl Bump {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Recipe { .. } => anyhow::bail!("bump recipe: not implemented"),
            Self::Pixi { .. } => anyhow::bail!("bump pixi: not implemented"),
            Self::Route { .. } => anyhow::bail!("bump route: not implemented"),
            Self::OpenPr { .. } => anyhow::bail!("bump open-pr: not implemented"),
        }
    }
}
