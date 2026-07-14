use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

/// semantic-release prepare-step callback: rewrite every non-self `path =` dep
/// in the *committed* consumer manifest to `"==<version>"`, reading the version
/// from the sibling's (already-bumped) pixi.toml. This converts opt-in path
/// coupling into a committed pin for exactly this release; the
/// `@semantic-release/git` asset commits the rewrite and the tag lands on it.
/// Runs after bump-pixi (topo order guarantees the sibling released first).
/// Not for direct use.
#[derive(Args, Debug)]
pub struct PinSiblings {
    /// pixi.toml of the package about to be released.
    #[arg(long)]
    pub pixi_toml: PathBuf,
}

impl PinSiblings {
    pub fn run(&self) -> Result<()> {
        let resolved = crate::commands::build_recipes::resolve_path_deps(&self.pixi_toml)?;
        for dep in &resolved {
            tracing::info!("pinned sibling {} = \"=={}\"", dep.name, dep.version);
        }
        if resolved.is_empty() {
            tracing::info!("{}: no path deps to pin", self.pixi_toml.display());
        }
        Ok(())
    }
}
