use super::*;
use crate::secret::ExposeSecret;

/// Typed empty argv; `&[]` alone can't infer the `AsRef<OsStr>` element type.
const NO_ARGS: [&str; 0] = [];

/// Run with a single in-memory sink, the shape most of these tests want.
fn teed(script: &str) -> (color_eyre::eyre::Result<()>, String) {
    let mut buf: Vec<u8> = Vec::new();
    let result = {
        let mut sink: &mut dyn Write = &mut buf;
        run_teed("sh", &["-c", script], std::slice::from_mut(&mut sink))
    };
    (result, String::from_utf8_lossy(&buf).into_owned())
}

#[test]
fn run_teed_sends_both_streams_to_the_sink() {
    let (result, written) = teed("echo to-stdout; echo to-stderr >&2");
    result.unwrap();
    assert!(written.contains("to-stdout"), "{written}");
    assert!(written.contains("to-stderr"), "{written}");
}

#[test]
fn run_teed_writes_every_sink() {
    let (mut first, mut second) = (Vec::new(), Vec::new());
    {
        let mut sinks: [&mut dyn Write; 2] = [&mut first, &mut second];
        run_teed("sh", &["-c", "echo both"], &mut sinks).unwrap();
    }
    assert_eq!(first, second);
    assert!(String::from_utf8_lossy(&first).contains("both"));
}

#[test]
fn run_teed_failure_quotes_the_tail() {
    let (result, _) = teed("echo the-real-reason >&2; exit 1");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("exited with"), "{msg}");
    assert!(msg.contains("the-real-reason"), "{msg}");
}

#[test]
fn a_long_output_is_quoted_from_its_end() {
    let (result, written) =
        teed("head -c 5000 /dev/zero | tr '\\0' 'x'; echo; echo LAST-LINE; exit 1");
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("LAST-LINE"), "{msg}");
    assert!(msg.contains('…'), "{msg}");
    // Bounded in the error only: the sink still got everything.
    assert!(written.len() > STDERR_LIMIT, "{}", written.len());
}

#[test]
fn run_teed_scrubs_a_token_the_subprocess_printed() {
    let token = crate::secret::Secret::new("tok-in-build-output");
    let (result, _) = teed(&format!("echo {} >&2; exit 1", token.expose_secret()));
    let msg = format!("{}", result.unwrap_err());
    assert!(!msg.contains("tok-in-build-output"), "{msg}");
    assert!(msg.contains(crate::secret::REDACTED), "{msg}");
}

// Losing the output is what this function exists to prevent, so a sink that
// cannot take it is an error rather than a silent gap.
#[test]
fn a_failing_sink_fails_the_call() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut broken = Broken;
    let mut sink: &mut dyn Write = &mut broken;
    let err = run_teed("echo", &["anything"], std::slice::from_mut(&mut sink)).unwrap_err();
    assert!(format!("{err:#}").contains("disk full"), "{err:#}");
}

#[test]
fn run_teed_accepts_a_file_sink() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.log");
    {
        let mut file = std::fs::File::create(&path).unwrap();
        let mut sink: &mut dyn Write = &mut file;
        run_teed("echo", &["to-file"], std::slice::from_mut(&mut sink)).unwrap();
    }
    assert!(
        std::fs::read_to_string(&path).unwrap().contains("to-file"),
        "file sink got nothing"
    );
}

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

// A caller that hands a credential straight to a subprocess never registers it,
// so the flag name is the only thing that marks the next argument as secret.
#[test]
fn a_credential_named_flag_redacts_its_value() {
    let labelled = label(
        "rattler-index",
        &[
            "azblob",
            "https://acct.blob.core.windows.net/general",
            "--sas-token",
            "skoid=abc&sig=PEeXeTR7O27VbQ%3D",
            "--max-parallel",
            "50",
        ],
    );
    assert!(!labelled.contains("sig="), "{labelled}");
    assert!(labelled.contains("--sas-token [REDACTED]"), "{labelled}");
    assert!(labelled.contains("--max-parallel 50"), "{labelled}");
}

#[test]
fn a_credential_named_flag_redacts_an_inline_value() {
    let labelled = label(
        "gh",
        &["auth", "--with-token=ghp_realtoken", "--hostname=x"],
    );
    assert!(!labelled.contains("ghp_realtoken"), "{labelled}");
    assert!(labelled.contains("--with-token=[REDACTED]"), "{labelled}");
    assert!(labelled.contains("--hostname=x"), "{labelled}");
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
