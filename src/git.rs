//! Git subprocess wrappers, built on [`crate::process`]. GitHub API calls
//! live in [`crate::gh`]; git reads credentials from the `insteadOf` rules
//! the caller's environment installs.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::consts::ORIGIN;
use crate::process;

/// `git clone --depth=1 --branch=<branch> <url> <dest>`.
pub fn shallow_clone(url: &str, branch: &str, dest: &Path) -> color_eyre::eyre::Result<()> {
    let branch_flag = format!("--branch={branch}");
    process::git(&[
        OsStr::new("clone"),
        OsStr::new("--depth=1"),
        OsStr::new(&branch_flag),
        OsStr::new(url),
        dest.as_os_str(),
    ])
}

/// `git fetch --depth=1 origin <branch>`, leaving the result in `FETCH_HEAD`
/// for a subsequent `checkout -b <branch> FETCH_HEAD`.
pub fn fetch_branch(dest: &Path, branch: &str) -> color_eyre::eyre::Result<()> {
    process::run_in(dest, "git", &["fetch", "--depth=1", ORIGIN, branch])
}

/// The `git diff --quiet` exit-code trichotomy, defined once: 0 means "no
/// difference", 1 means "there is one", and anything else means git itself
/// failed and neither answer is true. Returns `true` for "no difference".
fn diff_quiet(cwd: &Path, args: &[&OsStr]) -> color_eyre::eyre::Result<bool> {
    match process::status_code_in(cwd, "git", args)? {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        other => color_eyre::eyre::bail!(
            "`git {}` failed in {} ({})",
            args.iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
            cwd.display(),
            other.map_or_else(
                || "terminated by a signal".to_string(),
                |code| format!("exit code {code}")
            )
        ),
    }
}

/// Whether `path` is byte-identical between revs `from` and `to`.
pub fn is_clean(cwd: &Path, from: &str, to: &str, path: &Path) -> color_eyre::eyre::Result<bool> {
    diff_quiet(
        cwd,
        &[
            OsStr::new("diff"),
            OsStr::new("--quiet"),
            OsStr::new(from),
            OsStr::new(to),
            OsStr::new("--"),
            path.as_os_str(),
        ],
    )
}

/// Whether nothing is staged in the index relative to HEAD — `git commit`
/// fails in that case, so callers check first. Ignores untracked files: a
/// stray file left in the checkout is not a change to commit.
pub fn nothing_staged(cwd: &Path) -> color_eyre::eyre::Result<bool> {
    diff_quiet(
        cwd,
        &[
            OsStr::new("diff"),
            OsStr::new("--cached"),
            OsStr::new("--quiet"),
        ],
    )
}

/// All tags of the repo containing `cwd`.
pub fn tags(cwd: &Path) -> color_eyre::eyre::Result<Vec<String>> {
    Ok(process::capture_in(cwd, "git", &["tag", "--list"])?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Absolute path of the working-tree root containing `cwd`.
pub fn toplevel(cwd: &Path) -> color_eyre::eyre::Result<PathBuf> {
    Ok(PathBuf::from(
        process::capture_in(cwd, "git", &["rev-parse", "--show-toplevel"])?.trim(),
    ))
}

/// Configured URL of `remote` in the repo containing `cwd`, verbatim: it may
/// be either form git records, so callers parse it with
/// [`crate::types::GithubRepoUrl::parse_remote`].
pub fn remote_url(cwd: &Path, remote: &str) -> color_eyre::eyre::Result<String> {
    Ok(process::capture_in(
        cwd,
        "git",
        &["config", "--get", &format!("remote.{remote}.url")],
    )?
    .trim()
    .to_string())
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
