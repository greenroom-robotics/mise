use crate::manifest::Package;
use anyhow::Context;
use clap::Args;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct Test {
    /// Single package name (default: all packages under --package-dir).
    #[arg(long)]
    pub package: Option<crate::types::PackageName>,
    /// Directory containing per-package pixi workspaces.
    #[arg(long, default_value = "packages")]
    pub package_dir: PathBuf,
    /// Directory to collect JUnit XML test reports into.
    #[arg(long, default_value = "test-reports")]
    pub report_dir: PathBuf,
    /// A `<env>:<task>` pair to run per package, repeatable. Defaults to
    /// `default:test` when none are given — the package's `default` pixi
    /// environment (test deps come in via an `extras = ["test"]` self-dep).
    /// Pass multiple to fan out across environments, e.g.
    /// `--job default:test --job tests-boost:test --job lint:lint`.
    #[arg(long = "job")]
    pub jobs: Vec<String>,
    /// Opt out of lockfile-strict runs. By default `pixi run --locked` requires
    /// the committed pixi.lock to satisfy the manifest and fails on drift — the
    /// committed lock is the permanent state and CI should catch drift against
    /// it. Pass `--no-locked` to re-resolve from the manifest instead
    /// (conda-forge style) — useful when a lock predates a backend change.
    #[arg(long = "no-locked", action = clap::ArgAction::SetFalse, default_value_t = true)]
    pub locked: bool,
}

/// A single `<env>:<task>` unit of work run against a package.
struct Job {
    env: String,
    task: String,
}

/// Parse `env:task` pairs, defaulting to a single `default:test` job when empty.
fn parse_jobs(raw: &[String]) -> anyhow::Result<Vec<Job>> {
    if raw.is_empty() {
        return Ok(vec![Job {
            env: "default".into(),
            task: "test".into(),
        }]);
    }
    raw.iter()
        .map(|spec| {
            let (env, task) = spec.split_once(':').with_context(|| {
                format!("invalid --job {spec:?}: expected `<env>:<task>` (e.g. default:test)")
            })?;
            if env.is_empty() || task.is_empty() {
                anyhow::bail!("invalid --job {spec:?}: env and task must both be non-empty");
            }
            Ok(Job {
                env: env.to_string(),
                task: task.to_string(),
            })
        })
        .collect()
}

impl Test {
    pub fn run(self) -> anyhow::Result<()> {
        let jobs = parse_jobs(&self.jobs)?;
        let pkgs = crate::manifest::discover(&self.package_dir, self.package.as_ref())?;
        if pkgs.is_empty() {
            anyhow::bail!("no packages found under {}", self.package_dir.display());
        }
        let failed: Vec<String> = pkgs
            .iter()
            .flat_map(|pkg| jobs.iter().map(move |job| (pkg, job)))
            .map(|(pkg, job)| self.run_job(pkg, job))
            .filter_map(Result::transpose)
            .collect::<anyhow::Result<_>>()?;
        if !failed.is_empty() {
            anyhow::bail!("tests failed for: {}", failed.join(", "));
        }
        Ok(())
    }

    /// Run one `<env>:<task>` against one package and collect its JUnit XML.
    /// Returns the failure label when the job did not exit 0 — the exit code is
    /// the test result, not an error, so a failing job (including one killed by
    /// a signal, which ROS tests do manage) is reported alongside the others by
    /// the caller rather than abandoning the remaining packages.
    fn run_job(&self, pkg: &Package, job: &Job) -> anyhow::Result<Option<String>> {
        let pkg_dir = &pkg.dir;
        println!(
            "==> mise ci test :: {} [{}:{}]",
            pkg_dir.display(),
            job.env,
            job.task
        );
        // Lockfile satisfaction is strict by default; --no-locked opts out and
        // re-resolves from the manifest instead.
        let mut argv: Vec<&OsStr> = vec![OsStr::new("run")];
        if self.locked {
            argv.push(OsStr::new("--locked"));
        }
        argv.extend([
            OsStr::new("--manifest-path"),
            pkg.manifest_path.as_os_str(),
            OsStr::new("-e"),
            OsStr::new(&job.env),
            OsStr::new(&job.task),
        ]);
        let code = crate::process::status_code("pixi", &argv)?;
        // Collect reports after each job, namespaced by env, so variants that
        // share the same colcon `build/` (e.g. standalone vs Boost Asio) don't
        // overwrite each other's JUnit XML. Collect regardless of pass/fail so
        // failing-test XML is captured too.
        match collect_reports(pkg_dir, &self.report_dir, &job.env) {
            Ok(0) => eprintln!("    no JUnit XML found under {}/build", pkg_dir.display()),
            Ok(n) => println!(
                "    collected {n} report(s) into {}",
                self.report_dir.display()
            ),
            Err(e) => eprintln!("    failed to collect reports: {e:#}"),
        }
        Ok((code != Some(0)).then(|| format!("{} [{}:{}]", pkg_dir.display(), job.env, job.task)))
    }
}

/// Collect a package's JUnit XML test reports into `report_dir`.
///
/// Globs `<pkg_dir>/build/**/*.xml` (the standard colcon `test-result`
/// location) and copies each file to
/// `<report_dir>/<package-dir-name>/<env>/<path-relative-to-pkg_dir>`,
/// preserving the relative path so filenames never collide across packages or
/// across environments that share the same `build/` dir. Returns the number of
/// files copied.
fn collect_reports(pkg_dir: &Path, report_dir: &Path, env: &str) -> anyhow::Result<usize> {
    let build = pkg_dir.join("build");
    if !build.is_dir() {
        return Ok(0);
    }
    let pkg_name = pkg_dir
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_else(|| "package".into());
    let dest_root = report_dir.join(&pkg_name).join(env);

    let xml = find_xml(&build)?;
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

/// Recursively collect `*.xml` files under `dir`.
fn find_xml(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            // Skip CTest's native result dir: build/<pkg>/Testing/<stamp>/Test.xml
            // is a <Site> doc, not JUnit, and crashes dorny/test-reporter.
            if path.file_name().is_some_and(|n| n == "Testing") {
                continue;
            }
            out.extend(find_xml(&path)?);
        } else if path.extension().is_some_and(|e| e == "xml")
            && path.file_name().is_some_and(|n| n != "package.xml")
        {
            // package.xml is the ROS manifest, not a JUnit report; colcon copies
            // it into build/<pkg>/ and collecting it crashes dorny/test-reporter
            // the same way CTest's Test.xml does.
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "test_tests.rs"]
mod tests;
