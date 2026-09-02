//! Shared harness for the e2e characterization tests.
//!
//! Each test gets an [`E2e`] sandbox: a temp dir holding
//! - `bin/` — PATH-shim executables (`gh`, `pixi`, `npx`) prepended to PATH.
//!   Every invocation appends its argv (as one atomic log line) to a log file
//!   and replays canned stdout/exit codes registered via [`E2e::respond`].
//!   An invocation with no registered response fails loudly (exit 127) so a
//!   subprocess the test didn't expect can never be silently absorbed. Real
//!   `git` is NOT shimmed — tests use real git against local temp repos only.
//! - `home/` — a scratch HOME so nothing of the host user's config leaks in.
//! - `gitconfig` — the file `GIT_CONFIG_GLOBAL` points at: a deterministic
//!   identity, an unroutable HTTP proxy (so any real git network use fails
//!   instantly instead of escaping the sandbox), plus any per-test
//!   `insteadOf` rewrites that redirect would-be network URLs to local
//!   `file://` remotes.
//!
//! mise's in-process HTTP calls can't be shimmed on PATH (and git's
//! `http.proxy` never applies to them), so they get a [`FixtureServer`] on
//! 127.0.0.1 plus the `MISE_GITHUB_RAW_URL` override.
//!
//! Commands run with a cleared environment; only PATH, HOME, TMPDIR, the git
//! config overrides and per-test vars are present, so host git config
//! (including insteadOf rules) and stray GITHUB_*/GH_* tokens cannot leak in.

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Separator the shims put between argv items in the log (ASCII unit separator).
const ARG_SEP: char = '\u{1f}';
/// Stand-in for newlines inside a logged argv item (ASCII record separator),
/// so one invocation stays one log line even when an arg is multiline
/// (e.g. a PR body).
const NL_SEP: char = '\u{1e}';

/// The executables the harness shims. Registering a response via
/// [`E2e::respond`] names one of these, so a typo'd program name is
/// unrepresentable.
#[derive(Debug, Clone, Copy)]
pub enum Shim {
    Gh,
    Pixi,
    Npx,
}

impl Shim {
    const fn name(self) -> &'static str {
        match self {
            Self::Gh => "gh",
            Self::Pixi => "pixi",
            Self::Npx => "npx",
        }
    }
}

pub struct E2e {
    /// Owns the on-disk sandbox; dropped (and deleted) with the harness.
    _temp: TempDir,
    /// Canonicalized sandbox root — every derived path is symlink-free, so
    /// `strip_prefix` against process-reported paths (git toplevel, cwd)
    /// behaves the same on hosts where /tmp is a symlink.
    root: PathBuf,
    bin: PathBuf,
    log: PathBuf,
    responses: PathBuf,
    home: PathBuf,
    tmp: PathBuf,
    gitconfig: PathBuf,
}

const SHIM_SCRIPT: &str = r#"#!/bin/sh
prog=$(basename "$0")
line="$prog"
for a in "$@"; do
  line="$line$(printf '\037%s' "$a" | tr '\n' '\036')"
done
printf '%s\n' "$line" >> "${MISE_E2E_SHIM_LOG:?}"
dir="${MISE_E2E_SHIM_RESPONSES:?}"
san() { printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'; }
for key in "$prog-$(san "${1:-}")-$(san "${2:-}")" "$prog-$(san "${1:-}")" "$prog"; do
  if [ -f "$dir/$key.stdout" ] || [ -f "$dir/$key.exit" ] || [ -f "$dir/$key.stderr" ]; then
    [ -f "$dir/$key.stdout" ] && cat "$dir/$key.stdout"
    [ -f "$dir/$key.stderr" ] && cat "$dir/$key.stderr" >&2
    [ -f "$dir/$key.exit" ] && exit "$(cat "$dir/$key.exit")"
    exit 0
  fi
done
echo "unshimmed call: $line" >&2
exit 127
"#;

impl E2e {
    pub fn new() -> Self {
        let temp = TempDir::new().expect("create e2e temp dir");
        let root = temp.path().canonicalize().expect("canonicalize temp dir");
        let bin = root.join("bin");
        let responses = root.join("responses");
        let home = root.join("home");
        let tmp = root.join("tmp");
        let log = root.join("shim.log");
        let gitconfig = root.join("gitconfig");
        for d in [&bin, &responses, &home, &tmp] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(&log, "").unwrap();
        // http.proxy points at an unroutable port so any git command that
        // tries to reach a real http(s) remote fails immediately; file://
        // and local-path remotes are unaffected.
        //
        // uploadpack.allowAnySHA1InWant lets a local fixture remote serve a
        // fetch-by-SHA the way GitHub does, so `git::fetch_rev` can be
        // exercised against a `file://` redirect.
        fs::write(
            &gitconfig,
            "[user]\n\tname = e2e-test\n\temail = e2e-test@example.invalid\n\
             [init]\n\tdefaultBranch = main\n\
             [http]\n\tproxy = http://127.0.0.1:9\n\
             [uploadpack]\n\tallowAnySHA1InWant = true\n",
        )
        .unwrap();

        let e2e = Self {
            _temp: temp,
            root,
            bin,
            log,
            responses,
            home,
            tmp,
            gitconfig,
        };
        for prog in [Shim::Gh, Shim::Pixi, Shim::Npx] {
            e2e.write_shim(prog.name());
        }
        e2e
    }

    /// The canonicalized sandbox root; create per-test dirs under this.
    pub fn path(&self) -> &Path {
        &self.root
    }

    fn write_shim(&self, prog: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.bin.join(prog);
        fs::write(&path, SHIM_SCRIPT).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Rust mirror of the shim's `san()`: everything outside `[A-Za-z0-9._-]`
    /// becomes `_`.
    fn san(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "._-".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// The response-file stem for `prog` invoked with `args` (only the first
    /// two args key the lookup, matching the shim script).
    fn response_stem(&self, prog: Shim, args: &[&str]) -> PathBuf {
        let mut key = prog.name().to_string();
        for a in args.iter().take(2) {
            key.push('-');
            key.push_str(&Self::san(a));
        }
        self.responses.join(key)
    }

    fn response_exists(&self, prog: Shim, args: &[&str]) -> bool {
        let stem = self.response_stem(prog, args);
        stem.with_extension("stdout").exists() || stem.with_extension("exit").exists()
    }

    /// Canned stdout (and implicit exit 0) for `prog` invoked with a first
    /// arg pair matching `args`.
    pub fn respond(&self, prog: Shim, args: &[&str], stdout: &str) {
        let stem = self.response_stem(prog, args);
        fs::write(stem.with_extension("stdout"), stdout).unwrap();
    }

    /// Like [`respond`](Self::respond), but only if no response (stdout or
    /// exit code) is already registered — lets helpers install defaults that
    /// tests can override beforehand.
    pub fn respond_if_unset(&self, prog: Shim, args: &[&str], stdout: &str) {
        if !self.response_exists(prog, args) {
            self.respond(prog, args, stdout);
        }
    }

    /// Canned stderr for `prog` invoked with a first arg pair matching `args`.
    /// For the paths where mise classifies a subprocess failure by what it
    /// said, not just by its exit code.
    pub fn respond_stderr(&self, prog: Shim, args: &[&str], stderr: &str) {
        let stem = self.response_stem(prog, args);
        fs::write(stem.with_extension("stderr"), stderr).unwrap();
    }

    /// Canned exit code for `prog` invoked with a first arg pair matching
    /// `args` (stdout stays empty unless also registered).
    pub fn respond_exit(&self, prog: Shim, args: &[&str], code: i32) {
        let stem = self.response_stem(prog, args);
        fs::write(stem.with_extension("exit"), code.to_string()).unwrap();
    }

    /// All shim invocations so far, in order, as argv vectors
    /// (`["gh", "pr", "list", ...]`).
    pub fn shim_calls(&self) -> Vec<Vec<String>> {
        let text = fs::read_to_string(&self.log).unwrap_or_default();
        text.lines()
            .map(|l| l.split(ARG_SEP).map(|a| a.replace(NL_SEP, "\n")).collect())
            .collect()
    }

    /// A `mise` command with the sandbox environment applied: cleared env,
    /// shim bin dir first on PATH, isolated HOME/TMPDIR and git config.
    pub fn mise(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("mise").unwrap();
        cmd.env_clear();
        for (k, v) in self.env_vars() {
            cmd.env(k, v);
        }
        cmd
    }

    fn env_vars(&self) -> Vec<(&'static str, String)> {
        let mut path = self.bin.display().to_string();
        match std::env::var("PATH") {
            Ok(host) if !host.is_empty() => {
                path.push(':');
                path.push_str(&host);
            }
            _ => {}
        }
        vec![
            ("PATH", path),
            ("HOME", self.home.display().to_string()),
            ("TMPDIR", self.tmp.display().to_string()),
            ("GIT_CONFIG_GLOBAL", self.gitconfig.display().to_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".to_string()),
            ("MISE_E2E_SHIM_LOG", self.log.display().to_string()),
            (
                "MISE_E2E_SHIM_RESPONSES",
                self.responses.display().to_string(),
            ),
        ]
    }

    /// Run real `git` with the sandbox environment (deterministic identity,
    /// isolated config). Panics on failure; returns stdout.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = std::process::Command::new("git");
        cmd.env_clear();
        for (k, v) in self.env_vars() {
            cmd.env(k, v);
        }
        let out = cmd.args(args).current_dir(dir).output().expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} in {} failed: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `git init` + first commit of everything currently in `dir`. Returns the
    /// commit SHA.
    pub fn git_init_commit(&self, dir: &Path, msg: &str) -> String {
        self.git(dir, &["init", "--quiet", "-b", "main"]);
        self.git_commit_all(dir, msg)
    }

    /// Stage everything and commit. Returns the commit SHA.
    pub fn git_commit_all(&self, dir: &Path, msg: &str) -> String {
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "--quiet", "-m", msg]);
        self.git(dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// Redirect `url` (and anything under it) to a local path via a git
    /// `insteadOf` rule in the sandbox's global config, so commands that would
    /// clone/fetch/push over the network hit a local repo instead.
    pub fn git_redirect(&self, url: &str, local: &Path) {
        let mut cfg = fs::read_to_string(&self.gitconfig).unwrap();
        cfg.push_str(&format!(
            "[url \"file://{}\"]\n\tinsteadOf = {url}\n",
            local.display()
        ));
        fs::write(&self.gitconfig, cfg).unwrap();
    }
}

/// A throwaway HTTP server on 127.0.0.1 serving a fixed path → body map.
///
/// mise reaches GitHub's raw-content host with in-process `ureq` calls, which
/// no PATH shim can intercept. Pointing `MISE_GITHUB_RAW_URL` at one of these
/// is what makes those paths testable.
pub struct FixtureServer {
    base: String,
    addr: std::net::SocketAddr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FixtureServer {
    /// Serve `routes` (request path without query string → response body).
    /// Any other path answers 404.
    pub fn start(routes: std::collections::BTreeMap<String, String>) -> Self {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = std::sync::Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let Ok(clone) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(clone);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                // Drain the headers so the client's write side completes
                // before we answer.
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) if line.trim().is_empty() => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/")
                    .to_string();
                let response = match routes.get(&path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\
                         Content-Type: text/plain; charset=utf-8\r\n\
                         Connection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\
                             Connection: close\r\n\r\n"
                        .to_string(),
                };
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            addr,
            stop,
            handle: Some(handle),
        }
    }

    /// Value for `MISE_GITHUB_RAW_URL`.
    pub fn base_url(&self) -> &str {
        &self.base
    }
}

impl Drop for FixtureServer {
    /// Shut the accept loop down and reclaim the thread and the port. Without
    /// this each server outlives its test, and a suite that starts one per
    /// test leaks both for the life of the test binary.
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // The loop is parked in a blocking `accept`; one throwaway connection
        // wakes it so it can observe the flag.
        let _ = std::net::TcpStream::connect(self.addr);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Compare `actual` against the golden file at
/// `tests/e2e/fixtures/<rel>`. Set `UPDATE_GOLDENS=1` to (re)write goldens
/// instead of asserting.
pub fn assert_golden(actual: &str, rel: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/e2e/fixtures")
        .join(rel);
    if std::env::var("UPDATE_GOLDENS").as_deref() == Ok("1") {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run with UPDATE_GOLDENS=1 to create it",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "golden mismatch for {rel}; run with UPDATE_GOLDENS=1 to regenerate"
    );
}

/// Replace every occurrence of `path` in `text` with `<TMP>` so temp-dir
/// contents can be golden-compared.
pub fn normalize_path(text: &str, path: &Path) -> String {
    text.replace(&path.display().to_string(), "<TMP>")
}

/// Write `body` to `root/rel`, creating parent dirs.
pub fn write_file(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// A pixi.toml declaring `[package] name` (plus `extra` appended verbatim) —
/// the shape `packages::discover` treats as a releasable package.
pub fn package_pixi_toml(name: &str, extra: &str) -> String {
    format!(
        "[workspace]\nname = \"{name}\"\n\n[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{extra}"
    )
}

/// The value following `flag` in a recorded argv, if present.
pub fn flag_value<'a>(call: &'a [String], flag: &str) -> Option<&'a str> {
    call.iter()
        .position(|a| a == flag)
        .and_then(|i| call.get(i + 1))
        .map(String::as_str)
}
