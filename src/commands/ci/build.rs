use clap::Args;
use color_eyre::eyre::WrapErr;
use std::ffi::OsStr;
use std::path::PathBuf;

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
    pub target_platform: Option<String>,
}

impl Build {
    pub fn run(self) -> color_eyre::eyre::Result<()> {
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
        if pkgs.is_empty() {
            color_eyre::eyre::bail!("no packages found under {}", self.package_dir.display());
        }
        let out_dir = std::env::var("RUNNER_TEMP").map_or_else(
            |_| std::path::PathBuf::from("./output"),
            |t| std::path::PathBuf::from(t).join("conda-bld"),
        );
        std::fs::create_dir_all(&out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;

        for pkg in pkgs {
            let pkg_dir = &pkg.dir;
            println!("==> mise ci build :: {}", pkg_dir.display());
            let mut argv: Vec<&OsStr> = vec![
                OsStr::new("publish"),
                OsStr::new("--path"),
                pkg.manifest_path.as_os_str(),
                OsStr::new("--target-dir"),
                out_dir.as_os_str(),
            ];
            if let Some(plat) = &self.target_platform {
                argv.extend([OsStr::new("--target-platform"), OsStr::new(plat)]);
            }
            crate::process::run("pixi", &argv)
                .with_context(|| format!("pixi build failed for {}", pkg_dir.display()))?;
        }
        Ok(())
    }
}
