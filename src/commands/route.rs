use std::{collections::BTreeMap, path::PathBuf};

use clap::Args;

use crate::{
    gh,
    manifest::PackageManifest,
    repo::Repo,
    routing::{self, RoutingFile, RoutingRule, published_channels},
    types::{PackageName, RemoteChannel},
};

#[derive(Args, Debug)]
pub struct Route {
    package: Option<String>,

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
    pub fn run(self) -> anyhow::Result<()> {
        let repo = Repo::or_discover(self.repo_root)?;
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
                .collect::<anyhow::Result<Vec<_>>>()
        })?;

        let routing_file = match self.routing_file {
            Some(p) => RoutingFile::Explicit { routing_file: p },
            None => RoutingFile::RepoDefault {
                repo_root: repo.root().to_path_buf(),
            },
        };
        let rules = routing::load_rules(routing_file)?;

        if self.json {
            println!(
                "{}",
                as_json(packages.as_slice(), rules.as_slice(), &self.channel_url,)?
            );
        } else {
            println!(
                "{}",
                as_human_readable(packages.as_slice(), rules.as_slice(), &self.channel_url,)
            );
        }

        Ok(())
    }
}

fn package_channel_pairs<'a>(
    packages: &'a [PackageManifest],
    rules: &[RoutingRule],
    channel: &RemoteChannel,
) -> Vec<(&'a PackageName, Vec<RemoteChannel>)> {
    packages
        .iter()
        .map(|pkg| {
            (
                pkg.name(),
                published_channels(rules, channel, pkg.name(), pkg.version()),
            )
        })
        .collect::<Vec<_>>()
}

fn as_json(
    packages: &[PackageManifest],
    rules: &[RoutingRule],
    channel: &RemoteChannel,
) -> anyhow::Result<String> {
    let pairs: BTreeMap<_, _> = package_channel_pairs(packages, rules, channel)
        .into_iter()
        .map(|pair| {
            (
                pair.0,
                pair.1
                    .iter()
                    .map(|chan| chan.to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Ok(serde_json::to_string(&pairs)?)
}

fn as_human_readable(
    packages: &[PackageManifest],
    rules: &[RoutingRule],
    channel: &RemoteChannel,
) -> String {
    let pairs = package_channel_pairs(packages, rules, channel);
    pairs
        .iter()
        .map(|pair| {
            let (name, channels) = pair;
            let channels_string = channels
                .iter()
                .map(|chan| chan.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}: {channels_string}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
