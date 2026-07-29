use super::*;

#[test]
fn run_succeeds() {
    run("true", &[]).unwrap();
}

#[test]
fn run_propagates_failure() {
    let err = run("false", &[]).unwrap_err();
    assert!(format!("{err}").contains("exited with"));
}

#[test]
fn run_in_uses_cwd() {
    run_in(Path::new("/"), "ls", &["-d", "/"]).unwrap();
}

#[test]
fn run_in_failure_names_the_cwd() {
    let err = run_in(Path::new("/"), "false", &[]).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("exited with"), "{msg}");
    assert!(msg.contains("in /"), "{msg}");
}
