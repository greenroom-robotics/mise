use anyhow::Context;
use clap::Args;
use std::ffi::OsStr;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct Build {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// rattler-build target subdir.
    #[arg(long)]
    pub target_platform: Option<String>,
}

impl Build {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_deref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let out_dir = std::env::var("RUNNER_TEMP")
            .map(|t| std::path::PathBuf::from(t).join("conda-bld"))
            .unwrap_or_else(|_| std::path::PathBuf::from("./output"));
        std::fs::create_dir_all(&out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;

        for pkg in pkgs {
            let pkg_dir = &pkg.dir;
            println!("==> mise ci build :: {}", pkg_dir.display());
            let mut argv: Vec<&OsStr> = vec![
                OsStr::new("build"),
                OsStr::new("--path"),
                pkg.manifest_path.as_os_str(),
                OsStr::new("--output-dir"),
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
