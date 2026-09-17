//! The one place production code spawns subprocesses.
//!
//! Four shapes, by what the caller needs back:
//!
//! | want | non-zero exit is | function |
//! |---|---|---|
//! | nothing (stream output) | an error | [`run`] / [`run_in`] |
//! | nothing (output to your writers) | an error quoting its tail | [`run_teed`] |
//! | stdout | an error | [`capture_in`] |
//! | stdout *or* the failure | an answer | [`capture_probe`] / [`capture_probe_in`] |
//! | just the exit code | an answer | [`status_code`] / [`status_code_in`] |
//!
//! Every command label that reaches a log or an error goes through
//! [`crate::secret::scrub`], so a token embedded in an argument (a tokenized
//! clone URL, a `git config url.https://x-access-token:…` key) is redacted.
//! A credential that was never wrapped in [`crate::secret::Secret`] is not in
//! that registry, so the value of a credential-named flag is redacted on the
//! flag name alone as well.

use std::ffi::OsStr;
use std::fmt::Display;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use color_eyre::eyre::WrapErr;

use crate::secret;

/// How much of a failed command's stderr is quoted back in the error. Long
/// enough for a git/gh/pixi diagnostic, short enough not to bury the message.
const STDERR_LIMIT: usize = 2000;

/// A flag whose value is a credential, matched on its last hyphenated word so
/// that a vendor prefix (`--sas-token`, `--api-key`) needs no enumeration.
fn flag_hides_its_value(arg: &str) -> bool {
    let Some(name) = arg.strip_prefix('-') else {
        return false;
    };
    let name = name.trim_start_matches('-').to_ascii_lowercase();
    matches!(
        name.rsplit('-').next(),
        Some("token" | "secret" | "password" | "key")
    )
}

/// Human-readable `prog arg arg`, with any registered secret and the value of
/// any credential-named flag redacted, in both the `--token v` and `--token=v`
/// spellings. Lossy: a label is diagnostic text, and a non-UTF-8 argument must
/// not turn a runnable command into an error.
fn label(prog: &str, args: &[impl AsRef<OsStr>]) -> String {
    let mut s = prog.to_string();
    let mut value_is_secret = false;
    for a in args {
        let a = a.as_ref().to_string_lossy();
        s.push(' ');
        match a.split_once('=') {
            _ if value_is_secret => {
                s.push_str(secret::REDACTED);
                value_is_secret = false;
            }
            Some((flag, _)) if flag_hides_its_value(flag) => {
                s.push_str(flag);
                s.push('=');
                s.push_str(secret::REDACTED);
            }
            _ => {
                s.push_str(&a);
                value_is_secret = flag_hides_its_value(&a);
            }
        }
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
    cwd.map_or_else(
        || format!("`{label}` exited with {status}"),
        |d| format!("`{label}` exited with {status} (in {})", d.display()),
    )
}

/// Trim and bound a captured stderr for inclusion in an error message.
fn quote_stderr(stderr: &str) -> String {
    let mut trimmed = secret::scrub(stderr.trim());
    if let Some((cut, _)) = trimmed.char_indices().nth(STDERR_LIMIT) {
        trimmed.truncate(cut);
        trimmed.push('…');
    }
    trimmed
}

/// Run `prog` with `args`, inheriting this process's stdout/stderr so
/// subprocess output is visible in real time. Bails on non-zero exit.
pub fn run(prog: &str, args: &[impl AsRef<OsStr>]) -> color_eyre::eyre::Result<()> {
    run_inner(prog, args, None)
}

pub fn run_in(cwd: &Path, prog: &str, args: &[impl AsRef<OsStr>]) -> color_eyre::eyre::Result<()> {
    run_inner(prog, args, Some(cwd))
}

fn run_inner(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    cwd: Option<&Path>,
) -> color_eyre::eyre::Result<()> {
    let (mut cmd, label) = build(prog, args, cwd);
    let status = cmd.status().with_context(|| format!("spawn `{label}`"))?;
    if !status.success() {
        color_eyre::eyre::bail!("{}", failure_message(&label, status, cwd));
    }
    Ok(())
}

/// The last `STDERR_LIMIT` bytes to pass through, kept so a failure can quote
/// where a command ended without re-reading whatever the sinks did with it.
///
/// Bytes rather than chars: this sees arbitrary chunk boundaries, so it cannot
/// assume a chunk is whole UTF-8. Decoding is deferred to [`Self::quote`].
#[derive(Default)]
struct Tail {
    bytes: std::collections::VecDeque<u8>,
    dropped: bool,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend(chunk);
        while self.bytes.len() > STDERR_LIMIT {
            self.bytes.pop_front();
            self.dropped = true;
        }
    }

    /// The kept bytes, scrubbed, with a leading ellipsis when the command
    /// produced more than was kept. A partial character at the front is decoded
    /// lossily like any other captured output.
    fn quote(&self) -> String {
        let bytes: Vec<u8> = self.bytes.iter().copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        let mut out = secret::scrub(text.trim());
        if self.dropped && !out.is_empty() {
            out.insert(0, '…');
        }
        out
    }
}

/// Run `prog` with `args`, writing everything it prints to `sinks` instead of
/// this process's stdout/stderr. Bails on non-zero exit, quoting the tail.
///
/// For commands run concurrently, where inheriting would shred each one's
/// output across the others. The caller owns what the output becomes — a file,
/// a buffer, several at once — so changing that never reaches back into here.
///
/// Both of the child's streams share one pipe, so their relative order is the
/// order the child wrote them. Output is drained as it arrives rather than
/// collected at exit, so a long or hung command's progress reaches the sinks
/// while it runs and nothing is buffered beyond the quoted tail.
///
/// A failing sink fails the call: losing the output is the thing this exists to
/// prevent, so it is reported rather than swallowed.
pub fn run_teed(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    sinks: &mut [&mut dyn Write],
) -> color_eyre::eyre::Result<()> {
    let (mut cmd, label) = build(prog, args, None);
    let (mut reader, writer) = std::io::pipe().context("create output pipe")?;
    let errors = writer.try_clone().context("dup pipe handle")?;
    let mut child = cmd
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(errors))
        .spawn()
        .with_context(|| format!("spawn `{label}`"))?;

    // The child holds the only other write ends; ours must go or the read
    // below never sees EOF.
    drop(cmd);

    let mut tail = Tail::default();
    let mut buf = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut buf)
            .with_context(|| format!("read output of `{label}`"))?;
        if read == 0 {
            break;
        }
        let chunk = buf.get(..read).unwrap_or_default();
        tail.push(chunk);
        for sink in &mut *sinks {
            sink.write_all(chunk)
                .with_context(|| format!("write output of `{label}`"))?;
        }
    }
    for sink in sinks {
        sink.flush()
            .with_context(|| format!("flush output of `{label}`"))?;
    }

    let status = child
        .wait()
        .with_context(|| format!("wait for `{label}`"))?;
    if status.success() {
        return Ok(());
    }
    let mut msg = failure_message(&label, status, None);
    let quoted = tail.quote();
    if !quoted.is_empty() {
        msg.push('\n');
        msg.push_str(&quoted);
    }
    Err(color_eyre::eyre::eyre!("{msg}"))
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
    #[must_use]
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
pub fn capture_probe(prog: &str, args: &[impl AsRef<OsStr>]) -> color_eyre::eyre::Result<Captured> {
    capture_inner(prog, args, None)
}

pub fn capture_probe_in(
    cwd: &Path,
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> color_eyre::eyre::Result<Captured> {
    capture_inner(prog, args, Some(cwd))
}

fn capture_inner(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    cwd: Option<&Path>,
) -> color_eyre::eyre::Result<Captured> {
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
pub fn capture_in(
    cwd: &Path,
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> color_eyre::eyre::Result<String> {
    match capture_probe_in(cwd, prog, args)? {
        Captured::Output(stdout) => Ok(stdout),
        Captured::Failed { code, stderr } => {
            let status = code.map_or_else(|| "a signal".to_string(), |c| format!("code {c}"));
            let mut msg = failure_message(&label(prog, args), status, Some(cwd));
            let stderr = quote_stderr(&stderr);
            if !stderr.is_empty() {
                msg.push_str(": ");
                msg.push_str(&stderr);
            }
            Err(color_eyre::eyre::eyre!("{msg}"))
        }
    }
}

/// Run `prog` and return its exit code, inheriting stdout/stderr.
///
/// For the commands whose *code* is the answer rather than an error — `git diff
/// --quiet` reports "no difference" as 0 and "difference" as 1, and both are
/// successful outcomes. `None` means the process was terminated by a signal
/// and produced no code at all, which the type keeps distinct from every real
/// code.
pub fn status_code(
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> color_eyre::eyre::Result<Option<i32>> {
    status_code_inner(prog, args, None)
}

pub fn status_code_in(
    cwd: &Path,
    prog: &str,
    args: &[impl AsRef<OsStr>],
) -> color_eyre::eyre::Result<Option<i32>> {
    status_code_inner(prog, args, Some(cwd))
}

fn status_code_inner(
    prog: &str,
    args: &[impl AsRef<OsStr>],
    cwd: Option<&Path>,
) -> color_eyre::eyre::Result<Option<i32>> {
    let (mut cmd, label) = build(prog, args, cwd);
    let status = cmd.status().with_context(|| format!("spawn `{label}`"))?;
    Ok(status.code())
}

pub fn git(args: &[impl AsRef<OsStr>]) -> color_eyre::eyre::Result<()> {
    run("git", args)
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
