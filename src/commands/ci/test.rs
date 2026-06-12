use anyhow::Context;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct Test {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<String>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// ROS distro identifier (passed to pixi-env tasks).
    #[arg(long, default_value = "kilted")]
    pub ros_distro: String,
    /// Directory to collect JUnit XML test reports into.
    #[arg(long, default_value = "test-reports")]
    pub report_dir: PathBuf,
}

impl Test {
    pub fn run(self) -> anyhow::Result<()> {
        let pkgs =
            crate::commands::ci::packages::discover(&self.package_dir, self.package.as_deref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let mut failed = Vec::new();
        for pixi in pkgs {
            let pkg_dir = pixi.parent().unwrap();
            println!("==> mise ci test :: {}", pkg_dir.display());
            let status = std::process::Command::new("pixi")
                .arg("run")
                .arg("--manifest-path")
                .arg(&pixi)
                .arg("-e")
                .arg("tests")
                .arg("test")
                .status()
                .map_err(|e| anyhow::anyhow!("failed to spawn pixi: {e}"))?;
            // Collect reports regardless of pass/fail so failing-test XML is
            // captured too.
            match collect_reports(pkg_dir, &self.report_dir) {
                Ok(0) => eprintln!("    no JUnit XML found under {}/build", pkg_dir.display()),
                Ok(n) => println!(
                    "    collected {n} report(s) into {}",
                    self.report_dir.display()
                ),
                Err(e) => eprintln!("    failed to collect reports: {e:#}"),
            }
            if !status.success() {
                failed.push(pkg_dir.display().to_string());
            }
        }
        if !failed.is_empty() {
            anyhow::bail!("tests failed for: {}", failed.join(", "));
        }
        Ok(())
    }
}

/// Collect a package's JUnit XML test reports into `report_dir`.
///
/// Globs `<pkg_dir>/build/**/*.xml` (the standard colcon `test-result`
/// location) and copies each file to
/// `<report_dir>/<package-dir-name>/<path-relative-to-pkg_dir>`, preserving the
/// relative path so filenames never collide across or within packages. Returns
/// the number of files copied.
fn collect_reports(pkg_dir: &Path, report_dir: &Path) -> anyhow::Result<usize> {
    let build = pkg_dir.join("build");
    if !build.is_dir() {
        return Ok(0);
    }
    let pkg_name = pkg_dir
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "package".into());
    let dest_root = report_dir.join(&pkg_name);

    let mut xml = Vec::new();
    find_xml(&build, &mut xml)?;

    for src in &xml {
        let rel = src.strip_prefix(pkg_dir).unwrap_or(src);
        let dest = dest_root.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::copy(src, &dest)
            .with_context(|| format!("copying {} to {}", src.display(), dest.display()))?;
    }
    Ok(xml.len())
}

/// Recursively collect `*.xml` files under `dir` into `out`.
fn find_xml(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            find_xml(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "xml") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn collect_reports_copies_junit_xml_namespaced_by_package() {
        let tmp = TempDir::new().unwrap();
        let pkg_dir = tmp.path().join("packages/foo");
        let results = pkg_dir.join("build/foo/test_results/foo");
        fs::create_dir_all(&results).unwrap();
        fs::write(results.join("foo.gtest.xml"), "<testsuite/>").unwrap();
        // Non-XML files under build/ must be ignored.
        fs::write(pkg_dir.join("build/foo/other.txt"), "x").unwrap();

        let report_dir = tmp.path().join("test-reports");
        let n = collect_reports(&pkg_dir, &report_dir).unwrap();

        assert_eq!(n, 1);
        let dest = report_dir.join("foo/build/foo/test_results/foo/foo.gtest.xml");
        assert!(dest.exists(), "expected {} to exist", dest.display());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "<testsuite/>");
    }

    #[test]
    fn collect_reports_returns_zero_when_no_build_dir() {
        let tmp = TempDir::new().unwrap();
        let pkg_dir = tmp.path().join("packages/empty");
        fs::create_dir_all(&pkg_dir).unwrap();
        let report_dir = tmp.path().join("test-reports");
        let n = collect_reports(&pkg_dir, &report_dir).unwrap();
        assert_eq!(n, 0);
    }
}
