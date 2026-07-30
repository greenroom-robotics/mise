use super::*;

#[test]
fn debug_never_reveals_the_value() {
    let s = Secret::new("hunter2-display-case");
    assert!(!format!("{s:?}").contains("hunter2-display-case"));
}

#[test]
fn debug_output_is_exactly_the_scrub_marker() {
    // Not merely "contains" — a debug-printed secret and a scrubbed one have
    // to be the same string, or the module doc's claim is decoration.
    let s = Secret::new("hunter5-marker-case");
    assert_eq!(format!("{s:?}"), "[REDACTED]");
    assert_eq!(format!("{s:?}"), REDACTED);
    assert_eq!(scrub("hunter5-marker-case"), format!("{s:?}"));
}

#[test]
fn expose_secret_is_the_only_way_out() {
    let s = Secret::new("hunter3-expose-case");
    assert_eq!(s.expose_secret(), "hunter3-expose-case");
}

#[test]
fn constructing_a_secret_registers_it_for_scrubbing() {
    // The plaintext embedded in a larger string (as in a tokenized clone URL)
    // is what scrubbing exists for.
    let _s = Secret::new("hunter4-scrub-case");
    let url = "https://x-access-token:hunter4-scrub-case@github.com/o/r.git";
    let scrubbed = scrub(url);
    assert!(!scrubbed.contains("hunter4-scrub-case"), "{scrubbed}");
    assert_eq!(
        scrubbed,
        "https://x-access-token:[REDACTED]@github.com/o/r.git"
    );
}

#[test]
fn an_empty_secret_is_not_registered() {
    // Registering "" would make scrub() redact between every character.
    let _s = Secret::new("");
    assert_eq!(scrub("nothing to hide"), "nothing to hide");
}
