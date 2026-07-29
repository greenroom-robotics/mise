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
