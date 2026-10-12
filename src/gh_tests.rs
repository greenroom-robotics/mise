use super::*;

// --- token precedence -------------------------------------------------------

/// A lookup over a fixed table, so precedence is tested without touching the
/// process environment (which is shared and would race other tests).
fn lookup_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let table: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |var: &str| table.iter().find(|(k, _)| k == var).map(|(_, v)| v.clone())
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
