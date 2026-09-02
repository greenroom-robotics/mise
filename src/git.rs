//! Git subprocess wrappers, built on [`crate::process`]. GitHub API calls
//! live in [`crate::gh`]; git reads credentials from the `insteadOf` rule
//! `gh::ensure_git_auth` installs.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::consts::ORIGIN;
use crate::process;
use crate::types::Sha40;

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

/// Materialize exactly one commit of `url` in `dest`: init a fresh repo, point
/// it at the remote, fetch the single rev, check it out. The rev is usually a
/// tagged release commit no branch tip points at, which `clone --branch`
/// cannot fetch.
pub fn fetch_rev(dest: &Path, url: &str, rev: &Sha40) -> color_eyre::eyre::Result<()> {
    std::fs::create_dir_all(dest)?;
    process::run_in(dest, "git", &["init", "--quiet"])?;
    process::run_in(dest, "git", &["remote", "add", ORIGIN, url])?;
    process::run_in(
        dest,
        "git",
        &["fetch", "--depth=1", "--quiet", ORIGIN, rev.as_str()],
    )?;
    process::run_in(dest, "git", &["checkout", "--quiet", "FETCH_HEAD"])
}

/// Init and check out the repo's git submodules, recursively, in an existing
/// checkout.
///
/// Relies on the `insteadOf` rules `gh::ensure_git_auth` installs: they cover
/// both the https and `git@github.com:` remote forms, so private submodules
/// pinned by SSH URL in `.gitmodules` fetch with the same token. `--depth=1`
/// works for arbitrary pinned SHAs on GitHub (it allows direct SHA fetches).
pub fn submodule_update(dest: &Path) -> color_eyre::eyre::Result<()> {
    process::run_in(
        dest,
        "git",
        &["submodule", "update", "--init", "--recursive", "--depth=1"],
    )
}

/// Pull the LFS objects under `include` into an existing checkout.
///
/// `--exclude=` clears any `.lfsconfig` `fetchexclude`: this checkout is a
/// throwaway build workdir scoped to one entry's `include`, and a source
/// repo's `fetchexclude = *` would mean fetching nothing at all.
pub fn lfs_pull(dest: &Path, include: &str) -> color_eyre::eyre::Result<()> {
    let include = format!("--include={include}");
    process::run_in(dest, "git", &["lfs", "pull", &include, "--exclude="])
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
            match other {
                Some(code) => format!("exit code {code}"),
                None => "terminated by a signal".to_string(),
            }
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

/// Paths touched in `range` (`<a>..<b>` or `<a>...<b>`), relative to the
/// repo root.
pub fn changed_files(root: &Path, range: &str) -> color_eyre::eyre::Result<Vec<PathBuf>> {
    Ok(
        process::capture_in(root, "git", &["diff", "--name-only", range])?
            .lines()
            .map(PathBuf::from)
            .collect(),
    )
}

/// Whether `rev` names a commit object present in this checkout. False for a
/// commit that exists upstream but was never fetched — the normal state of a
/// shallow CI checkout.
pub fn rev_exists(root: &Path, rev: &Sha40) -> color_eyre::eyre::Result<bool> {
    let spec = format!("{}^{{commit}}", rev.as_str());
    Ok(
        process::capture_probe_in(root, "git", &["cat-file", "-e", &spec])?
            .output()
            .is_some(),
    )
}

/// `git show <rev>:<path>` → file content, or `None` if the path did not exist
/// at that rev (e.g. a newly added file).
///
/// The rev itself is checked first, because `git show` cannot tell the two
/// failures apart and the callers read `None` as "this file is new". On a
/// shallow checkout (`actions/checkout` defaults to `fetch-depth: 1`) a rev
/// that is perfectly real upstream is simply absent locally; silently
/// answering "new file" there turns missing history into a wrong-but-green
/// full rebuild, so it is an error instead.
pub fn file_at_rev(
    root: &Path,
    rev: &Sha40,
    path: &str,
) -> color_eyre::eyre::Result<Option<String>> {
    color_eyre::eyre::ensure!(
        rev_exists(root, rev)?,
        "commit {} is not present in {} — the checkout is too shallow to read \
         {path} at that rev (fetch more history, e.g. actions/checkout with \
         fetch-depth: 0)",
        rev.as_str(),
        root.display(),
    );
    let spec = format!("{}:{path}", rev.as_str());
    Ok(process::capture_probe_in(root, "git", &["show", &spec])?.output())
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod tests;
