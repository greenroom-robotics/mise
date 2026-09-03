//! GitHub-facing plumbing: workflow events, auth, the REST/raw HTTP calls, the
//! `gh` CLI wrappers, and the Actions output/summary files.
//!
//! Git subprocess work lives in [`crate::git`].

use std::env;
use std::fmt::Display;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use color_eyre::eyre::WrapErr;
use serde::Deserialize;

use crate::consts::DEFAULT_BRANCH;
use crate::manifest::PackageManifest;
use crate::process;
use crate::repo::Repo;
use crate::secret::{ExposeSecret, Secret};
use crate::types::{PixiNativeEntry, Sha40};

/// The GitHub "no parent" sentinel for `push` event's `before` field on initial pushes.
const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    PullRequest { base: Sha40, head: Sha40 },
    Push { before: Option<Sha40>, after: Sha40 },
    WorkflowDispatch,
    Other,
}

impl Event {
    /// The SHA to diff the working tree against for change detection.
    #[must_use]
    pub const fn base_sha(&self) -> Option<&Sha40> {
        match self {
            Self::PullRequest { base, .. } => Some(base),
            Self::Push {
                before: Some(b), ..
            } => Some(b),
            _ => None,
        }
    }

    /// Load using `GITHUB_EVENT_NAME` + `GITHUB_EVENT_PATH`.
    pub fn load() -> color_eyre::eyre::Result<Self> {
        let name = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
        if name == "workflow_dispatch" {
            return Ok(Self::WorkflowDispatch);
        }
        let path = env::var("GITHUB_EVENT_PATH").context("GITHUB_EVENT_PATH must be set")?;
        Self::load_from(&name, Path::new(&path))
    }

    pub fn load_from(name: &str, path: &Path) -> color_eyre::eyre::Result<Self> {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_str_with_kind(name, &text)
    }

    pub fn from_str_with_kind(name: &str, json: &str) -> color_eyre::eyre::Result<Self> {
        match name {
            "pull_request" => {
                #[derive(Deserialize)]
                struct E {
                    pull_request: Pr,
                }
                #[derive(Deserialize)]
                struct Pr {
                    base: Side,
                    head: Side,
                }
                #[derive(Deserialize)]
                struct Side {
                    sha: Sha40,
                }
                let e: E = serde_json::from_str(json)?;
                Ok(Self::PullRequest {
                    base: e.pull_request.base.sha,
                    head: e.pull_request.head.sha,
                })
            }
            "push" => {
                #[derive(Deserialize)]
                struct E {
                    before: String,
                    after: Sha40,
                }
                let e: E = serde_json::from_str(json)?;
                let before = if e.before == ZERO_SHA {
                    None
                } else {
                    Some(Sha40::new(e.before)?)
                };
                Ok(Self::Push {
                    before,
                    after: e.after,
                })
            }
            "workflow_dispatch" => Ok(Self::WorkflowDispatch),
            _ => Ok(Self::Other),
        }
    }

    /// The `git diff` range that expresses "what this event changed", or
    /// `None` when the event carries no base to diff from (which callers read
    /// as "assume everything changed").
    #[must_use]
    pub fn diff_range(&self) -> Option<String> {
        match self {
            Self::PullRequest { base, head } => Some(format!("{base}...{head}")),
            Self::Push {
                before: Some(b),
                after,
            } => Some(format!("{b}..{after}")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChangedFiles {
    All,
    Paths(Vec<PathBuf>),
}

/// Environment variables holding a GitHub credential, in precedence order:
///
/// 1. `API_TOKEN_GITHUB` — the cross-repo App/PAT token, the only one with
///    write access to *other* repositories.
/// 2. `GH_TOKEN` — the `gh` CLI's own variable, set explicitly per step.
/// 3. `GITHUB_TOKEN` — the automatic per-job token, scoped to this repository.
const TOKEN_VARS: [&str; 3] = ["API_TOKEN_GITHUB", "GH_TOKEN", "GITHUB_TOKEN"];

/// The GitHub token: the first of [`TOKEN_VARS`] set to a non-empty value.
/// An empty variable is skipped, since CI commonly exports a variable with no
/// value when a secret is unavailable.
#[must_use]
pub fn token() -> Option<Secret> {
    token_from(|var| env::var(var).ok())
}

fn token_from(lookup: impl Fn(&str) -> Option<String>) -> Option<Secret> {
    TOKEN_VARS
        .into_iter()
        .find_map(|var| lookup(var).filter(|t| !t.is_empty()))
        .map(Secret::new)
}

/// The `insteadOf` rule mise installs is keyed on a URL that embeds the token,
/// so a new token means a new config key rather than a new value for the old
/// one. This prefix identifies the whole family for removal.
const INSTEAD_OF_PREFIX: &str = "url.https://x-access-token:";

/// Config keys from `git config --get-regexp` output that are mise-installed
/// `insteadOf` rules. Git lowercases the variable name (`insteadof`) but keeps
/// the URL subsection verbatim, so match accordingly.
fn stale_instead_of_keys(get_regexp_output: &str) -> Vec<String> {
    get_regexp_output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|key| {
            key.starts_with(INSTEAD_OF_PREFIX) && key.to_ascii_lowercase().ends_with(".insteadof")
        })
        .map(str::to_string)
        .collect()
}

/// Teach `git` to authenticate GitHub HTTPS remotes with [`token`], by writing
/// an `insteadOf` rewrite into the global git config.
///
/// Every previously-installed rule is removed first: the token is part of the
/// config *key*, so a rotated token writes a second key rather than replacing
/// the first, and on a long-lived runner revoked tokens would accumulate as
/// competing rules for the same prefix.
pub fn ensure_git_auth() -> color_eyre::eyre::Result<()> {
    let existing = process::capture_probe(
        "git",
        &[
            "config",
            "--global",
            "--get-regexp",
            "^url\\.https://x-access-token:",
        ],
    )?
    .output()
    .unwrap_or_default();
    for key in stale_instead_of_keys(&existing) {
        process::git(&["config", "--global", "--unset-all", &key])?;
    }

    if let Some(t) = token() {
        let key = format!(
            "{INSTEAD_OF_PREFIX}{}@github.com/.insteadOf",
            t.expose_secret()
        );
        process::git(&["config", "--global", &key, "https://github.com/"])?;
        // Also claim the SSH remote form, mapped straight to the token URL:
        // git applies insteadOf rewrites once (no chaining), so an ssh→https
        // rewrite would not then pick up the rule above.
        process::git(&["config", "--global", "--add", &key, "git@github.com:"])?;
    }
    Ok(())
}

/// An override base URL, normalized to no trailing slash; callers supply the
/// leading `/` of the path.
fn base_url(var: &str, default: &str) -> String {
    let raw = env::var(var).unwrap_or_else(|_| default.to_string());
    raw.trim_end_matches('/').to_string()
}

/// Base URL for raw file content; `MISE_GITHUB_RAW_URL` overrides it.
fn raw_base() -> String {
    base_url("MISE_GITHUB_RAW_URL", "https://raw.githubusercontent.com")
}

/// The URL to clone `owner/repo` from: tokenized HTTPS when a token is
/// available, ssh otherwise. The token is registered for log scrubbing, so
/// the URL is safe to hand to [`crate::process`].
#[must_use]
pub fn clone_url(repo: &str) -> String {
    token().map_or_else(
        || format!("git@github.com:{repo}.git"),
        |t| {
            format!(
                "https://x-access-token:{}@github.com/{repo}.git",
                t.expose_secret()
            )
        },
    )
}

/// Fetch one file's content from the raw-content host at a given rev.
pub fn fetch_raw_file(
    owner: &str,
    repo: &str,
    rev: &str,
    path: &str,
) -> color_eyre::eyre::Result<String> {
    let url = format!("{}/{owner}/{repo}/{rev}/{path}", raw_base());
    let token = token();
    let mut req = ureq::get(&url);
    if let Some(t) = &token {
        req = req.set("Authorization", &format!("Bearer {}", t.expose_secret()));
    }
    match req.call() {
        Ok(resp) => resp
            .into_string()
            .with_context(|| format!("read body of {url}")),
        Err(ureq::Error::Status(code, _)) => {
            let hint = if token.is_none() && (code == 401 || code == 403 || code == 404) {
                " (set GH_TOKEN for private repos)"
            } else {
                ""
            };
            color_eyre::eyre::bail!("failed to fetch {url} ({code}){hint}")
        }
        Err(e) => color_eyre::eyre::bail!("fetch {url}: {e}"),
    }
}

/// Fetch and parse an entry's `pixi.toml` at its pinned rev without cloning.
pub fn fetch_upstream_manifest(
    entry: &PixiNativeEntry,
) -> color_eyre::eyre::Result<PackageManifest> {
    let text = fetch_raw_file(
        entry.url.owner(),
        entry.url.repo(),
        entry.rev.as_str(),
        &entry.manifest_rel_path(),
    )
    .with_context(|| format!("entry {}", entry.name))?;
    PackageManifest::parse(&text)
        .with_context(|| format!("entry {}: parse upstream pixi.toml", entry.name))
}

/// Paths the event touched, or [`ChangedFiles::All`] when nothing can be diffed.
///
/// That is the case when the event carries no base ref, or a base that is not
/// present locally (a force-push rewrote it away); a full rebuild is the
/// fail-safe.
pub fn changed_files(repo: &Repo, event: &Event) -> color_eyre::eyre::Result<ChangedFiles> {
    if let Some(base) = event.base_sha()
        && !crate::git::rev_exists(repo.root(), base)?
    {
        tracing::warn!("base {base} not present locally; rebuilding all");
        return Ok(ChangedFiles::All);
    }
    match event.diff_range() {
        None => Ok(ChangedFiles::All),
        Some(range) => Ok(ChangedFiles::Paths(crate::git::changed_files(
            repo.root(),
            &range,
        )?)),
    }
}

/// A pull request identified the way every `gh pr` subcommand identifies one:
/// the repository it lives in plus the head branch.
#[derive(Debug, Clone, Copy)]
pub struct PrRef<'a> {
    repo: &'a str,
    branch: &'a str,
}

impl<'a> PrRef<'a> {
    pub fn new(repo: &'a str, branch: &'a str) -> color_eyre::eyre::Result<Self> {
        let mut parts = repo.split('/');
        let ok = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(owner), Some(name), None)
                if !owner.is_empty()
                    && !name.is_empty()
                    && !repo.contains(char::is_whitespace)
        );
        color_eyre::eyre::ensure!(ok, "recipes repo must be `owner/repo`, got {repo:?}");
        color_eyre::eyre::ensure!(!branch.is_empty(), "PR branch must not be empty");
        Ok(Self { repo, branch })
    }

    /// `--repo <owner/repo>`, spliced into every subcommand's argv.
    const fn repo_flag(&self) -> [&'a str; 2] {
        ["--repo", self.repo]
    }

    #[must_use]
    pub const fn branch(&self) -> &'a str {
        self.branch
    }
}

/// The `gh` CLI wrappers. Every call passes `--repo` explicitly, so `gh`
/// never infers the target from a checkout.
pub mod pr {
    use super::{DEFAULT_BRANCH, PrRef, process};

    /// `gh pr <verb>` with `--repo` guaranteed present.
    fn argv<'a>(verb: &'a str, pr: PrRef<'a>, rest: &[&'a str]) -> Vec<&'a str> {
        let mut argv = vec!["pr", verb];
        argv.extend(pr.repo_flag());
        argv.extend_from_slice(rest);
        argv
    }

    /// Open a PR from the ref's branch onto the default branch.
    pub fn create(pr: PrRef<'_>, title: &str, body: &str) -> color_eyre::eyre::Result<()> {
        process::run(
            "gh",
            &argv(
                "create",
                pr,
                &[
                    "--base",
                    DEFAULT_BRANCH,
                    "--head",
                    pr.branch(),
                    "--title",
                    title,
                    "--body",
                    body,
                ],
            ),
        )
    }

    /// Refresh an existing PR's title and body.
    pub fn edit(pr: PrRef<'_>, title: &str, body: &str) -> color_eyre::eyre::Result<()> {
        process::run(
            "gh",
            &argv("edit", pr, &[pr.branch(), "--title", title, "--body", body]),
        )
    }

    /// Enable GitHub native auto-merge (squash) so the PR lands once CI passes.
    pub fn merge_auto(pr: PrRef<'_>) -> color_eyre::eyre::Result<()> {
        process::run(
            "gh",
            &argv("merge", pr, &[pr.branch(), "--auto", "--squash"]),
        )
    }

    /// URL of the PR for this branch. `None` if `gh` fails or prints nothing —
    /// callers treat the link as best-effort.
    #[must_use]
    pub fn view_url(pr: PrRef<'_>) -> Option<String> {
        // `pr view` takes the branch first, so this one argv is hand-built.
        let mut args = vec!["pr", "view", pr.branch()];
        args.extend(pr.repo_flag());
        args.extend(["--json", "url", "--jq", ".url"]);
        let out = process::capture_probe("gh", &args).ok()?.output()?;
        let url = out.trim().to_string();
        (!url.is_empty()).then_some(url)
    }

    /// Body of the open PR for this branch, or `None` if there is no such PR.
    /// Doubles as the PR-exists check: `None` means no PR, distinct from a PR
    /// with an empty body.
    #[must_use]
    pub fn list_body(pr: PrRef<'_>) -> Option<String> {
        let out = process::capture_probe(
            "gh",
            &argv("list", pr, &["--head", pr.branch(), "--json", "body"]),
        )
        .ok()?
        .output()?;
        let prs: Vec<serde_json::Value> = serde_json::from_str(&out).ok()?;
        let first = prs.first()?;
        Some(first["body"].as_str().unwrap_or_default().to_string())
    }
}

pub mod outputs {
    use super::{Display, Path, WrapErr, Write, env, fs};

    /// Append `key=value` to `$GITHUB_OUTPUT`. No-op when unset.
    pub fn set(key: &str, value: &impl Display) -> color_eyre::eyre::Result<()> {
        let Some(path) = env::var_os("GITHUB_OUTPUT") else {
            return Ok(());
        };
        let formatted = format!("{value}");
        color_eyre::eyre::ensure!(
            !formatted.contains('\n'),
            "outputs::set value must not contain newlines (multiline values need the heredoc format); key={key}"
        );
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open $GITHUB_OUTPUT ({})", Path::new(&path).display()))?;
        writeln!(f, "{key}={formatted}").context("write $GITHUB_OUTPUT line")?;
        Ok(())
    }
}

pub mod summary {
    use super::{Write, env, fs};

    /// Append a Markdown section to `$GITHUB_STEP_SUMMARY`. Best-effort and
    /// infallible: a missing variable (local run) or an unwritable file must
    /// never fail the command whose result is being summarized.
    pub fn append(md: &str) {
        let Some(path) = env::var_os("GITHUB_STEP_SUMMARY") else {
            return;
        };
        if let Ok(mut f) = fs::OpenOptions::new().append(true).open(path) {
            let _ = writeln!(f, "{md}");
        }
    }
}

#[cfg(test)]
#[path = "gh_tests.rs"]
mod tests;
