use std::path::PathBuf;

use anyhow::Context;
use clap::Subcommand;

use crate::repo::Repo;

#[derive(Subcommand, Debug)]
pub enum Snapshot {
    /// Refresh rosdistro_snapshot.yaml and the vinca-cache repodata.
    Refresh {
        #[arg(long)]
        repo_root: Option<PathBuf>,
    },
}

impl Snapshot {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Self::Refresh { repo_root } => refresh(repo_root),
        }
    }
}

const SNAPSHOT_URL: &str =
    "https://raw.githubusercontent.com/RoboStack/ros-kilted/main/rosdistro_snapshot.yaml";
const CHANNEL_BASE: &str = "https://prefix.dev/robostack-kilted";
const SUBDIRS: &[&str] = &["linux-64", "noarch"];

fn refresh(repo_root: Option<PathBuf>) -> anyhow::Result<()> {
    let repo = Repo::or_discover(repo_root)?;
    let root = repo.root();

    let snapshot_path = root.join("rosdistro_snapshot.yaml");
    download_to(SNAPSHOT_URL, &snapshot_path)?;
    tracing::info!("Refreshed {}", snapshot_path.display());

    let cache_root = root.join(".vinca-cache");
    for subdir in SUBDIRS {
        let dir = cache_root.join(subdir);
        std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
        let url = format!("{CHANNEL_BASE}/{subdir}/repodata.json");
        let dest = dir.join("repodata.json");
        download_to(&url, &dest)?;
        tracing::info!("Refreshed {}", dest.display());
    }

    Ok(())
}

fn download_to(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let resp = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let bytes = {
        let mut buf = Vec::new();
        std::io::copy(&mut resp.into_reader(), &mut buf)
            .with_context(|| format!("read body of {url}"))?;
        buf
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    std::fs::write(dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
    Ok(())
}
