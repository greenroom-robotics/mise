use clap::Args;
use color_eyre::eyre::WrapErr;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::local_deps::LocalDeps;
use crate::types::{Arch, LocalChannel};

#[derive(Args, Debug)]
pub struct Build {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<crate::types::PackageName>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// rattler-build target subdir.
    #[arg(long)]
    pub target_platform: Option<Arch>,
}

enum PublishTarget<'a> {
    Dir(&'a Path),
    Channel(&'a LocalChannel),
}

impl PublishTarget<'_> {
    fn args(&self) -> [OsString; 2] {
        match self {
            Self::Dir(dir) => ["--target-dir".into(), dir.into()],
            Self::Channel(ch) => ["--target-channel".into(), ch.to_string().into()],
        }
    }
}

fn publish(
    manifest: &Path,
    target: &PublishTarget<'_>,
    target_platform: Option<Arch>,
) -> color_eyre::eyre::Result<()> {
    let mut argv: Vec<OsString> = vec!["publish".into(), "--path".into(), manifest.into()];
    argv.extend(target.args());
    if let Some(plat) = target_platform {
        argv.extend(["--target-platform".into(), plat.to_string().into()]);
    }
    crate::process::run("pixi", &argv)
}

impl Build {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
        if pkgs.is_empty() {
            color_eyre::eyre::bail!("no packages found under {}", self.package_dir.display());
        }
        let base = std::env::var_os("RUNNER_TEMP").map_or_else(|| "./output".into(), PathBuf::from);
        let out_dir = base.join("conda-bld");
        std::fs::create_dir_all(&out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;
        let channel = LocalChannel::fresh(&base.join("local-deps"))?;

        let target_platform = self.target_platform;
        let mut local_deps = LocalDeps::new(channel.clone(), |manifest| {
            publish(manifest, &PublishTarget::Channel(&channel), target_platform)
        });

        for pkg in pkgs {
            let pkg_dir = &pkg.dir;
            println!("==> mise ci build :: {}", pkg_dir.display());
            local_deps
                .prepare(&pkg)
                .with_context(|| format!("preparing path deps for {}", pkg_dir.display()))?;
            publish(
                &pkg.manifest_path,
                &PublishTarget::Dir(&out_dir),
                target_platform,
            )
            .with_context(|| format!("pixi build failed for {}", pkg_dir.display()))?;
        }
        Ok(())
    }
}
