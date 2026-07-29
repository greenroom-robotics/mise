use std::env;
use std::fmt::Display;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use serde::Deserialize;

use crate::repo::Repo;
use crate::types::Sha40;

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
    pub fn base_sha(&self) -> Option<&Sha40> {
        match self {
            Event::PullRequest { base, .. } => Some(base),
            Event::Push {
                before: Some(b), ..
            } => Some(b),
            _ => None,
        }
    }

    /// Load using `GITHUB_EVENT_NAME` + `GITHUB_EVENT_PATH`.
    pub fn load() -> anyhow::Result<Self> {
        let name = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
        if name == "workflow_dispatch" {
            return Ok(Event::WorkflowDispatch);
        }
        let path = env::var("GITHUB_EVENT_PATH").context("GITHUB_EVENT_PATH must be set")?;
        Self::load_from(&name, Path::new(&path))
    }

    pub fn load_from(name: &str, path: &Path) -> anyhow::Result<Self> {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_str_with_kind(name, &text)
    }

    pub fn from_str_with_kind(name: &str, json: &str) -> anyhow::Result<Self> {
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
                Ok(Event::PullRequest {
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
                Ok(Event::Push {
                    before,
                    after: e.after,
                })
            }
            "workflow_dispatch" => Ok(Event::WorkflowDispatch),
            _ => Ok(Event::Other),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChangedFiles {
    All,
    Paths(Vec<PathBuf>),
}

/// Workflow whose last green run marks how far the channel has been published.
const PUBLISH_WORKFLOW: &str = "publish.yml";

/// `head_sha` of the most recent **successful** publish run on `main`, or
/// `None` if there has never been one. This — not the push's own `before` — is
/// the correct base for change detection: `publish.yml` runs with
/// `cancel-in-progress: false`, so a run that is superseded while queued never
/// builds its range. Diffing from `before` would drop those changes
/// permanently; diffing from the last green publish folds them into the next
/// run. Querying for `status=success` cannot match the current run (still
/// in-progress), so there is no self-exclusion to worry about.
fn last_successful_publish_sha() -> anyhow::Result<Option<Sha40>> {
    let repo = env::var("GITHUB_REPOSITORY").context("GITHUB_REPOSITORY must be set")?;
    let token = env::var("GITHUB_TOKEN")
        .or_else(|_| env::var("GH_TOKEN"))
        .context("GITHUB_TOKEN or GH_TOKEN must be set to query workflow runs")?;
    let url = format!(
        "https://api.github.com/repos/{repo}/actions/workflows/{PUBLISH_WORKFLOW}/runs\
         ?branch=main&status=success&per_page=1"
    );
    let body: serde_json::Value = ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "ros-recipes-mise")
        .call()
        .context("query publish workflow runs")?
        .into_json()
        .context("parse workflow runs response")?;
    let Some(sha) = body["workflow_runs"]
        .get(0)
        .and_then(|r| r["head_sha"].as_str())
    else {
        return Ok(None);
    };
    Ok(Some(Sha40::new(sha)?))
}

/// Replace a push event's `before` with the last green publish SHA so change
/// detection diffs from the real channel high-water mark (see
/// [`last_successful_publish_sha`]). `None` base — no prior success, or the API
/// lookup failed — degrades to a full rebuild downstream, matching the existing
/// "no base ref → build everything" fail-safe. Non-push events pass through.
pub fn rebase_push_to_last_publish(event: Event) -> anyhow::Result<Event> {
    let Event::Push { after, .. } = event else {
        return Ok(event);
    };
    let before = match last_successful_publish_sha() {
        Ok(sha) => sha,
        Err(e) => {
            tracing::warn!("could not resolve last successful publish run ({e:#}); rebuilding all");
            None
        }
    };
    Ok(Event::Push { before, after })
}

pub fn changed_files(repo: &Repo, event: &Event) -> anyhow::Result<ChangedFiles> {
    let range = match event {
        Event::PullRequest { base, head } => format!("{base}...{head}"),
        Event::Push {
            before: Some(b),
            after,
        } => format!("{b}..{after}"),
        _ => return Ok(ChangedFiles::All),
    };
    let out = Command::new("git")
        .args(["diff", "--name-only", &range])
        .current_dir(repo.root())
        .output()
        .context("run git diff")?;
    if !out.status.success() {
        anyhow::bail!("git diff failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let paths = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(PathBuf::from)
        .collect();
    Ok(ChangedFiles::Paths(paths))
}

/// `git show <rev>:<path>` → file content, or `None` if the path did not exist
/// at that rev (e.g. a newly added file).
pub fn file_at_rev(repo: &Repo, rev: &Sha40, path: &str) -> anyhow::Result<Option<String>> {
    let spec = format!("{}:{path}", rev.as_str());
    let out = Command::new("git")
        .args(["show", &spec])
        .current_dir(repo.root())
        .output()
        .context("run git show")?;
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
    } else {
        Ok(None)
    }
}

pub mod outputs {
    use super::*;

    /// Append `key=value` to `$GITHUB_OUTPUT`. No-op when unset.
    pub fn set(key: &str, value: &impl Display) -> anyhow::Result<()> {
        let Some(path) = env::var_os("GITHUB_OUTPUT") else {
            return Ok(());
        };
        let formatted = format!("{value}");
        anyhow::ensure!(
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

#[cfg(test)]
#[path = "gh_tests.rs"]
mod tests;
