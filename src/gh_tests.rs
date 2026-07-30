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
fn rebase_passes_through_non_push_events() {
    let pr = Event::PullRequest {
        base: Sha40::new("1111111111111111111111111111111111111111").unwrap(),
        head: Sha40::new("2222222222222222222222222222222222222222").unwrap(),
    };
    assert_eq!(rebase_push_to_last_publish(pr.clone()).unwrap(), pr);
    assert_eq!(
        rebase_push_to_last_publish(Event::WorkflowDispatch).unwrap(),
        Event::WorkflowDispatch
    );
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
    // SAFETY: mutates process-global env; nextest runs each test in its
    // own process so this cannot race other tests.
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

// --- token precedence -------------------------------------------------------

/// A lookup over a fixed table, so precedence is tested without touching the
/// process environment (which is shared and would race other tests).
fn lookup_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let table: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |var: &str| {
        table
            .iter()
            .find(|(k, _)| k == var)
            .map(|(_, v)| v.to_string())
    }
}

/// Fixture tokens are deliberately distinctive strings rather than words like
/// `gh` or `github`. Constructing a [`Secret`] registers its plaintext with the
/// process-wide scrub registry for the life of the process, and `scrub` is a
/// substring replace — a token of `gh` would redact those two letters out of
/// every command label any later test in the same process logs.
fn token_of(pairs: &[(&str, &str)]) -> Option<String> {
    token_from(lookup_of(pairs)).map(|t| t.expose_secret().to_string())
}

#[test]
fn token_precedence_is_api_token_then_gh_then_github() {
    assert_eq!(
        token_of(&[
            ("API_TOKEN_GITHUB", "tok-from-api-var"),
            ("GH_TOKEN", "tok-from-gh-var"),
            ("GITHUB_TOKEN", "tok-from-github-var"),
        ]),
        Some("tok-from-api-var".into())
    );
    assert_eq!(
        token_of(&[
            ("GH_TOKEN", "tok-from-gh-var"),
            ("GITHUB_TOKEN", "tok-from-github-var")
        ]),
        Some("tok-from-gh-var".into())
    );
    assert_eq!(
        token_of(&[("GITHUB_TOKEN", "tok-from-github-var")]),
        Some("tok-from-github-var".into())
    );
    assert_eq!(token_of(&[]), None);
}

// CI routinely exports a variable with no value when a secret is unavailable;
// an empty winner would mean "authenticated with the empty string".
#[test]
fn an_empty_token_variable_is_skipped_rather_than_winning() {
    assert_eq!(
        token_of(&[("API_TOKEN_GITHUB", ""), ("GH_TOKEN", "tok-from-gh-var")]),
        Some("tok-from-gh-var".into())
    );
    assert_eq!(token_of(&[("GH_TOKEN", "")]), None);
}

#[test]
fn a_token_is_wrapped_so_it_cannot_be_formatted_into_a_message() {
    let t = token_from(lookup_of(&[("GH_TOKEN", "tok-gh-fmt-case")])).unwrap();
    assert!(!format!("{t:?}").contains("tok-gh-fmt-case"));
}

// --- insteadOf cleanup ------------------------------------------------------

// The token is part of the config *key*, so a rotated token writes a new key
// instead of replacing the old one. Every one of ours has to be found.
#[test]
fn stale_instead_of_keys_finds_every_previously_written_rule() {
    let output = "\
url.https://x-access-token:old1@github.com/.insteadof https://github.com/
url.https://x-access-token:old2@github.com/.insteadof https://github.com/
";
    assert_eq!(
        stale_instead_of_keys(output),
        vec![
            "url.https://x-access-token:old1@github.com/.insteadof",
            "url.https://x-access-token:old2@github.com/.insteadof",
        ]
    );
}

#[test]
fn stale_instead_of_keys_leaves_unrelated_config_alone() {
    let output = "\
url.file:///tmp/bare.insteadof https://github.com/o/r
url.https://x-access-token:t@github.com/.pushinsteadof https://github.com/
user.name someone
";
    assert!(stale_instead_of_keys(output).is_empty());
    assert!(stale_instead_of_keys("").is_empty());
}

// --- PrRef ------------------------------------------------------------------

#[test]
fn pr_ref_rejects_anything_that_is_not_owner_slash_repo() {
    assert!(PrRef::new("owner/repo", "b").is_ok());
    for bad in ["repo", "a/b/c", "/repo", "owner/", "own er/repo", ""] {
        assert!(PrRef::new(bad, "b").is_err(), "{bad:?} should be rejected");
    }
    assert!(PrRef::new("owner/repo", "").is_err());
}

// Every `gh pr` subcommand must be scoped to the recipes repo, never to
// whatever repo the runner happens to be checked out in.
#[test]
fn pr_ref_always_yields_the_repo_flag() {
    let pr = PrRef::new("owner/repo", "release/x").unwrap();
    assert_eq!(pr.repo_flag(), ["--repo", "owner/repo"]);
    assert_eq!(pr.branch(), "release/x");
}

// --- base URLs --------------------------------------------------------------

#[test]
fn a_base_url_override_never_produces_a_double_slash() {
    // SAFETY: single-threaded within this test; the var is read immediately.
    unsafe { std::env::set_var("MISE_TEST_BASE_URL", "http://127.0.0.1:9/") };
    assert_eq!(
        base_url("MISE_TEST_BASE_URL", "unused"),
        "http://127.0.0.1:9"
    );
    unsafe { std::env::remove_var("MISE_TEST_BASE_URL") };
    assert_eq!(base_url("MISE_TEST_BASE_URL", "https://x/"), "https://x");
}
