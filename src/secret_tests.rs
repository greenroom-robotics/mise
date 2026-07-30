use super::*;

#[test]
fn display_and_debug_never_reveal_the_value() {
    let s = Secret::new("hunter2-display-case");
    assert_eq!(format!("{s}"), REDACTED);
    assert_eq!(format!("{s:?}"), "Secret(***)");
    assert!(!format!("{s} {s:?}").contains("hunter2"));
}

#[test]
fn expose_is_the_only_way_out() {
    let s = Secret::new("hunter3-expose-case");
    assert_eq!(s.expose(), "hunter3-expose-case");
}

#[test]
fn constructing_a_secret_registers_it_for_scrubbing() {
    // The plaintext embedded in a larger string (as in a tokenized clone URL)
    // is what scrubbing exists for.
    let _s = Secret::new("hunter4-scrub-case");
    let url = "https://x-access-token:hunter4-scrub-case@github.com/o/r.git";
    let scrubbed = scrub(url);
    assert!(!scrubbed.contains("hunter4-scrub-case"), "{scrubbed}");
    assert_eq!(scrubbed, "https://x-access-token:***@github.com/o/r.git");
}

#[test]
fn an_empty_secret_is_not_registered() {
    // Registering "" would make scrub() redact between every character.
    let _s = Secret::new("");
    assert_eq!(scrub("nothing to hide"), "nothing to hide");
}
