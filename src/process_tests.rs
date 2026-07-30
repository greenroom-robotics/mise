use super::*;
use crate::secret::ExposeSecret;

/// Typed empty argv; `&[]` alone can't infer the `AsRef<OsStr>` element type.
const NO_ARGS: [&str; 0] = [];

#[test]
fn run_succeeds() {
    run("true", &NO_ARGS).unwrap();
}

#[test]
fn run_propagates_failure() {
    let err = run("false", &NO_ARGS).unwrap_err();
    assert!(format!("{err}").contains("exited with"));
}

#[test]
fn run_in_uses_cwd() {
    run_in(Path::new("/"), "ls", &["-d", "/"]).unwrap();
}

#[test]
fn run_in_failure_names_the_cwd() {
    let err = run_in(Path::new("/"), "false", &NO_ARGS).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("exited with"), "{msg}");
    assert!(msg.contains("in /"), "{msg}");
}

#[test]
fn capture_returns_stdout_verbatim() {
    // Not trimmed: `echo` adds the newline and it survives.
    assert_eq!(capture_in(Path::new("/"), "echo", &["hi"]).unwrap(), "hi\n");
}

#[test]
fn capture_accepts_path_args_without_utf8_laundering() {
    let dir = Path::new("/");
    let out = capture_in(Path::new("/"), "ls", &[OsStr::new("-d"), dir.as_os_str()]).unwrap();
    assert_eq!(out.trim(), "/");
}

#[test]
fn capture_in_failure_names_the_cwd() {
    let err = capture_in(Path::new("/"), "false", &NO_ARGS).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("exited with"), "{msg}");
    assert!(msg.contains("in /"), "{msg}");
}

// A bare exit status says a command failed but not why; the subprocess's own
// stderr is the whole diagnosis, so it has to survive into the error.
#[test]
fn capture_in_failure_quotes_the_subprocess_stderr() {
    let err = capture_in(
        Path::new("/"),
        "sh",
        &["-c", "echo 'the actual reason' >&2; exit 3"],
    )
    .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("the actual reason"), "{msg}");
    assert!(msg.contains("code 3"), "{msg}");
}

#[test]
fn capture_probe_reports_the_failure_instead_of_erroring() {
    let captured = capture_probe("sh", &["-c", "echo boom >&2; exit 4"]).unwrap();
    let Captured::Failed { code, stderr } = captured else {
        panic!("expected a failure")
    };
    assert_eq!(code, Some(4));
    assert_eq!(stderr.trim(), "boom");
}

#[test]
fn capture_probe_output_collapses_any_failure_to_none() {
    assert_eq!(
        capture_probe("echo", &["hi"]).unwrap().output().as_deref(),
        Some("hi\n")
    );
    assert_eq!(capture_probe("false", &NO_ARGS).unwrap().output(), None);
}

#[test]
fn status_code_reports_nonzero_without_erroring() {
    assert_eq!(status_code("true", &NO_ARGS).unwrap(), Some(0));
    assert_eq!(status_code("false", &NO_ARGS).unwrap(), Some(1));
}

#[test]
fn status_code_is_none_when_a_signal_killed_the_process() {
    // Not representable as a code, so the type says so rather than inventing
    // a sentinel a caller could mistake for a real exit status.
    assert_eq!(
        status_code("sh", &["-c", "kill -TERM $$"]).unwrap(),
        None,
        "signal-terminated process must not report a code"
    );
}

#[test]
fn status_code_in_uses_cwd() {
    assert_eq!(
        status_code_in(Path::new("/"), "true", &NO_ARGS).unwrap(),
        Some(0)
    );
}

#[test]
fn spawn_failure_is_reported_as_such() {
    let err = capture_probe("mise-no-such-program-exists", &NO_ARGS).unwrap_err();
    assert!(format!("{err:#}").contains("spawn"), "{err:#}");
}

// The reason `Secret` registers its plaintext: by the time a tokenized clone
// URL reaches the subprocess it is an ordinary String, so redaction has to
// happen where the label is built.
#[test]
fn a_token_embedded_in_an_argument_never_reaches_the_error_message() {
    let token = crate::secret::Secret::new("tok-process-label-case");
    let url = format!(
        "https://x-access-token:{}@github.com/o/r.git",
        token.expose_secret()
    );
    let err = capture_in(Path::new("/"), "false", &[url.as_str()]).unwrap_err();
    let msg = format!("{err:#}");
    assert!(!msg.contains("tok-process-label-case"), "{msg}");
    assert!(msg.contains(crate::secret::REDACTED), "{msg}");
}
