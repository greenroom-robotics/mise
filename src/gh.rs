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
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn parses_pull_request_event() {
        let json = include_str!("../tests/fixtures/event_pull_request.json");
        let e = Event::from_str_with_kind("pull_request", json).unwrap();
        let Event::PullRequest { base, head } = e else {
            panic!("expected PR")
        };
        assert_eq!(base.as_str(), "0000000000000000000000000000000000000001");
        assert_eq!(head.as_str(), "0000000000000000000000000000000000000002");
    }

    #[test]
    fn parses_push_event() {
        let json = include_str!("../tests/fixtures/event_push.json");
        let e = Event::from_str_with_kind("push", json).unwrap();
        let Event::Push { before, after } = e else {
            panic!("expected Push")
        };
        assert_eq!(
            before.unwrap().as_str(),
            "0000000000000000000000000000000000000003"
        );
        assert_eq!(after.as_str(), "0000000000000000000000000000000000000004");
    }

    #[test]
    fn parses_push_event_with_zero_before() {
        let json = r#"{"before":"0000000000000000000000000000000000000000","after":"0000000000000000000000000000000000000004"}"#;
        let e = Event::from_str_with_kind("push", json).unwrap();
        let Event::Push { before, .. } = e else {
            panic!()
        };
        assert!(before.is_none());
    }

    #[test]
    fn parses_push_event_rejects_short_zero_before() {
        let json = r#"{"before":"00","after":"0000000000000000000000000000000000000004"}"#;
        assert!(Event::from_str_with_kind("push", json).is_err());
    }

    #[test]
    fn parses_workflow_dispatch() {
        assert_eq!(
            Event::from_str_with_kind("workflow_dispatch", "{}").unwrap(),
            Event::WorkflowDispatch,
        );
    }

    #[test]
    fn outputs_set_writes_line() {
        let tmp = NamedTempFile::new().unwrap();
        // SAFETY: single-threaded test (run with --test-threads=1)
        unsafe {
            env::set_var("GITHUB_OUTPUT", tmp.path());
        }
        outputs::set("foo", &"bar").unwrap();
        outputs::set("count", &7u32).unwrap();
        let contents = fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(contents, "foo=bar\ncount=7\n");
        unsafe {
            env::remove_var("GITHUB_OUTPUT");
        }
    }

    #[test]
    fn outputs_set_is_noop_when_env_unset() {
        unsafe {
            env::remove_var("GITHUB_OUTPUT");
        }
        outputs::set("foo", &"bar").unwrap(); // no panic, no file
    }

    #[test]
    fn outputs_set_rejects_multiline_value() {
        let tmp = NamedTempFile::new().unwrap();
        unsafe {
            env::set_var("GITHUB_OUTPUT", tmp.path());
        }
        let err = outputs::set("k", &"line1\nline2").unwrap_err();
        assert!(format!("{err:#}").contains("newlines"));
        unsafe {
            env::remove_var("GITHUB_OUTPUT");
        }
    }
}
