//! GitHub-facing plumbing: auth, the `gh` CLI wrappers, and the Actions step
//! summary file.
//!
//! Git subprocess work lives in [`crate::git`].

use std::env;
use std::fs;
use std::io::Write;

use crate::consts::DEFAULT_BRANCH;
use crate::process;
use crate::secret::{ExposeSecret, Secret};

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
