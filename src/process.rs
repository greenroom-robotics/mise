//! The one place production code spawns subprocesses. (Two test files still
//! build their own `Command` — `verify_siblings_tests.rs` needs per-invocation
//! `GIT_*` identity env, and `internal_action_refs.rs` runs outside the lib —
//! neither is a code path that ships.)
//!
//! Three shapes, by what the caller needs back:
//!
//! | want | non-zero exit is | function |
//! |---|---|---|
//! | nothing (stream output) | an error | [`run`] / [`run_in`] |
//! | stdout | an error | [`capture_in`] |
//! | stdout *or* the failure | an answer | [`capture_probe`] / [`capture_probe_in`] |
//! | just the exit code | an answer | [`status_code`] / [`status_code_in`] |
//!
//! Arguments are `AsRef<OsStr>`, so `&str`, `String`, `Path`, `PathBuf` and
//! `OsStr` all work directly — callers never launder a path through
//! `to_str().unwrap()` and risk a panic on a non-UTF-8 path.
//!
//! Every command label that reaches a log or an error goes through
//! [`crate::secret::scrub`], so a token embedded in an argument (a tokenized
//! clone URL, a `git config url.https://x-access-token:…` key) is redacted.

use std::ffi::OsStr;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;

use anyhow::Context;

use crate::secret;

/// How much of a failed command's stderr is quoted back in the error. Long
/// enough for a git/gh/pixi diagnostic, short enough not to bury the message.
const STDERR_LIMIT: usize = 2000;

/// Human-readable `prog arg arg`, with any registered secret redacted. Lossy
/// on purpose: a label is diagnostic text, and a non-UTF-8 argument must not
/// turn a perfectly runnable command into an error.
fn label(prog: &str, args: &[impl AsRef<OsStr>]) -> String {
    let mut s = prog.to_string();
    for a in args {
        s.push(' ');
        s.push_str(&a.as_ref().to_string_lossy());
    }
    secret::scrub(&s)
}

fn build(prog: &str, args: &[impl AsRef<OsStr>], cwd: Option<&Path>) -> (Command, String) {
    let mut cmd = Command::new(prog);
    cmd.args(args.iter().map(AsRef::as_ref));
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let label = label(prog, args);
    tracing::info!(target: "mise::process", "{label}");
    (cmd, label)
}

/// A non-zero exit, reported with the working directory when one was set —
/// a failure inside a temp checkout is undiagnosable without it.
fn failure_message(label: &str, status: impl Display, cwd: Option<&Path>) -> String {
    match cwd {
        Some(d) => format!("`{label}` exited with {status} (in {})", d.display()),
        None => format!("`{label}` exited with {status}"),
    }
}

/// Trim and bound a captured stderr for inclusion in an error message.
fn quote_stderr(stderr: &str) -> String {
    let trimmed = secret::scrub(stderr.trim());
    match trimmed.char_indices().nth(STDERR_LIMIT) {
        Some((cut, _)) => format!("{}…", &trimmed[..cut]),
        None => trimmed,
    }
}

/// Run `prog` with `args`, inheriting this process's stdout/stderr so
/// subprocess output is visible in real time. Bails on non-zero exit.
pub fn run(prog: &str, args: &[impl AsRef<OsStr>]) -> anyhow::Result<()> {
    run_inner(prog, args, None)
}

pub fn run_in(cwd: &Path, prog: &str, args: &[impl AsRef<OsStr>]) -> anyhow::Result<()> {
    run_inner(prog, args, Some(cwd))
}

fn run_inner(prog: &str, args: &[impl AsRef<OsStr>], cwd: Option<&Path>) -> anyhow::Result<()> {
    let (mut cmd, label) = build(prog, args, cwd);
    let status = cmd.status().with_context(|| format!("spawn `{label}`"))?;
    if !status.success() {
        anyhow::bail!("{}", failure_message(&label, status, cwd));
    }
    Ok(())
}

/// What a captured command produced. `Failed` carries the subprocess's own
/// explanation, so a caller that treats failure as an answer can still
/// classify *why* it failed rather than guessing.
#[derive(Debug)]
pub enum Captured {
    Output(String),
    Failed {
        /// `None` when the process was terminated by a signal.
        code: Option<i32>,
        stderr: String,
    },
}

impl Captured {
    /// The stdout, or `None` for any failure — the shape a probe wants when
    /// "it didn't work" and "there is nothing there" are the same answer.
    pub fn output(self) -> Option<String> {
        match self {
            Self::Output(s) => Some(s),
            Self::Failed { .. } => None,
        }
    }
}

/// Run `prog` and hand back stdout or the failure, whichever happened. `Err`
/// is reserved for the command not running at all.
///
/// Two decoding rules, applied to every captured command in the codebase:
///
/// - **Output is decoded lossily.** Captured output is git/gh/pixi text that
///   is ASCII in practice, but it can carry a path byte-for-byte from the
///   filesystem (`git diff --name-only`, `git tag --list`). A single
///   non-UTF-8 byte in one filename must not fail the whole command, so
///   invalid sequences become U+FFFD rather than an error.
/// - **Stdout is returned verbatim, never trimmed.** Some callers capture
///   *file contents* (`git show <rev>:<path>`), where a trailing newline is
///   part of the value. Callers wanting a single-line answer trim it.
pub fn capture_probe(prog: &str, args: &[impl AsRef<OsStr>]) -> anyhow::Result<Captured> {
    capture_inner(prog, args, None)
}

pub fn capture_probe_in(
    cwd: &Path,
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> anyhow::Result<Captured> {
    capture_inner(prog, args, Some(cwd))
}

fn capture_inner(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    cwd: Option<&Path>,
) -> anyhow::Result<Captured> {
    let (mut cmd, label) = build(prog, args, cwd);
    let out = cmd.output().with_context(|| format!("spawn `{label}`"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        tracing::debug!(
            target: "mise::process",
            "{}: {}",
            failure_message(&label, out.status, cwd),
            quote_stderr(&stderr),
        );
        return Ok(Captured::Failed {
            code: out.status.code(),
            stderr,
        });
    }
    Ok(Captured::Output(
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// Run `prog` in `cwd` and return its stdout, bailing on a non-zero exit.
///
/// Stderr is captured rather than inherited, and the failure message quotes it
/// (trimmed and bounded) — an exit status alone says a command failed but not
/// why, and for these callers the subprocess's own words are the whole
/// diagnosis. Decoding follows the rules on [`capture_probe`].
pub fn capture_in(cwd: &Path, prog: &str, args: &[impl AsRef<OsStr>]) -> anyhow::Result<String> {
    match capture_probe_in(cwd, prog, args)? {
        Captured::Output(stdout) => Ok(stdout),
        Captured::Failed { code, stderr } => {
            let status = match code {
                Some(c) => format!("code {c}"),
                None => "a signal".to_string(),
            };
            let mut msg = failure_message(&label(prog, args), status, Some(cwd));
            let stderr = quote_stderr(&stderr);
            if !stderr.is_empty() {
                msg.push_str(&format!(": {stderr}"));
            }
            Err(anyhow::anyhow!("{msg}"))
        }
    }
}

/// Run `prog` and return its exit code, inheriting stdout/stderr. For the
/// commands whose *code* is the answer rather than an error — `git diff
/// --quiet` reports "no difference" as 0 and "difference" as 1, and both are
/// successful outcomes. `None` means the process was terminated by a signal
/// and produced no code at all, which the type keeps distinct from every real
/// code rather than encoding as a sentinel.
///
/// Kept as a pair despite one caller each (`git`'s shared `diff --quiet`
/// trichotomy, and `ci test`): the alternative is a second module building its
/// own `Command`, which is what this module exists to prevent.
pub fn status_code(prog: &str, args: &[impl AsRef<OsStr>]) -> anyhow::Result<Option<i32>> {
    status_code_inner(prog, args, None)
}

pub fn status_code_in(
    cwd: &Path,
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> anyhow::Result<Option<i32>> {
    status_code_inner(prog, args, Some(cwd))
}

fn status_code_inner(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    cwd: Option<&Path>,
) -> anyhow::Result<Option<i32>> {
    let (mut cmd, label) = build(prog, args, cwd);
    let status = cmd.status().with_context(|| format!("spawn `{label}`"))?;
    Ok(status.code())
}

pub fn git(args: &[impl AsRef<OsStr>]) -> anyhow::Result<()> {
    run("git", args)
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
