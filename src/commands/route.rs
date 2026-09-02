use std::{collections::BTreeMap, path::PathBuf};

use clap::Args;

use crate::{
    gh,
    manifest::PackageManifest,
    repo::Repo,
    routing::{self, RoutingFile, published_channels, published_channels_from_filename},
    types::RemoteChannel,
};

#[derive(Args, Debug)]
pub struct Route {
    package: Option<String>,

    #[arg(long, requires = "package")]
    is_file: bool,

    #[arg(long, short)]
    routing_file: Option<PathBuf>,

    #[arg(long, short)]
    packages_file: Option<PathBuf>,

    #[arg(long)]
    channel_url: RemoteChannel,

    #[arg(long)]
    repo_root: Option<PathBuf>,

    #[arg(long)]
    json: bool,
}

impl Route {
    /// Run the `route` command
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let repo = Repo::or_discover(self.repo_root)?;
        let routing_file = match self.routing_file {
            Some(p) => RoutingFile::Explicit { routing_file: p },
            None => RoutingFile::RepoDefault {
                repo_root: repo.root().to_path_buf(),
            },
        };
        let rules = routing::load_rules(routing_file)?;

        let pairs: Vec<(String, Vec<RemoteChannel>)> = if self.is_file {
            let filename = self.package.expect("clap: --is-file requires package");
            let channels = published_channels_from_filename(&rules, &self.channel_url, &filename);
            vec![(filename, channels)]
        } else {
            let manifest = repo.pixi_native_manifest()?;
            let package = self.package.as_ref();
            let packages: Vec<PackageManifest> = std::thread::scope(|scope| {
                let handles: Vec<_> = manifest
                    .packages
                    .iter()
                    .filter(|pkg| package.is_none_or(|name| pkg.name.as_str() == name))
                    .map(|pkg| scope.spawn(move || gh::fetch_upstream_manifest(pkg)))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("fetch thread panicked"))
                    .collect::<color_eyre::eyre::Result<Vec<_>>>()
            })?;
            packages
                .iter()
                .map(|pkg| {
                    (
                        pkg.name().to_string(),
                        published_channels(&rules, &self.channel_url, pkg.name(), pkg.version()),
                    )
                })
                .collect()
        };

        if self.json {
            println!("{}", as_json(&pairs)?);
        } else {
            println!("{}", as_human_readable(&pairs));
        }

        Ok(())
    }
}

fn as_json(pairs: &[(String, Vec<RemoteChannel>)]) -> color_eyre::eyre::Result<String> {
    let map: BTreeMap<_, _> = pairs
        .iter()
        .map(|(name, channels)| {
            (
                name,
                channels
                    .iter()
                    .map(RemoteChannel::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Ok(serde_json::to_string(&map)?)
}

fn as_human_readable(pairs: &[(String, Vec<RemoteChannel>)]) -> String {
    pairs
        .iter()
        .map(|(name, channels)| {
            let channels_string = channels
                .iter()
                .map(RemoteChannel::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}: {channels_string}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
