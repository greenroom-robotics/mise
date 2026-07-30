use super::*;

/// The span's own text. Tests only — production callers splice with
/// [`ByteSpan::before`] / [`ByteSpan::after`] and never need the middle.
fn spanned<'a>(span: &ByteSpan, text: &'a str) -> &'a str {
    &text[span.0.clone()]
}

const ROSDISTRO: &str = "\
# leading comment
foo_pkg:
  url: https://example.invalid/foo
  tag: 1.0.0

bar_pkg:
  url: https://example.invalid/bar
";

#[test]
fn section_bounds_covers_the_key_line_and_its_indented_body() {
    let r = section_bounds(ROSDISTRO, "foo_pkg").unwrap();
    assert_eq!(
        spanned(&r, ROSDISTRO),
        "foo_pkg:\n  url: https://example.invalid/foo\n  tag: 1.0.0\n\n"
    );
    // The blank line before the next key belongs to the block, and the next
    // key starts exactly where it ends.
    assert_eq!(
        r.after(ROSDISTRO),
        "bar_pkg:\n  url: https://example.invalid/bar\n"
    );
}

#[test]
fn section_bounds_runs_to_eof_for_the_last_block() {
    let r = section_bounds(ROSDISTRO, "bar_pkg").unwrap();
    assert_eq!(r.after(ROSDISTRO), "");
}

#[test]
fn section_bounds_ignores_indented_and_valued_keys() {
    assert!(section_bounds(ROSDISTRO, "url").is_none());
    assert!(section_bounds("foo_pkg: inline\n", "foo_pkg").is_none());
}

// ---------------------------------------------------------------------------
// Column-0 comments: the one rule, exercised from both traversals
// ---------------------------------------------------------------------------

/// A comment run with nothing indented after it captions the block below, so
/// it ends the block above.
const CAPTION: &str = "pkg:\n  url: u\n# unrelated\nother:\n  url: v\n";

#[test]
fn a_captioning_comment_ends_the_block() {
    let r = section_bounds(CAPTION, "pkg").unwrap();
    assert_eq!(spanned(&r, CAPTION), "pkg:\n  url: u\n");
    // …and the reader agrees: the comment and everything after it are outside.
    assert_eq!(field_of(CAPTION, "pkg", "url"), Some("u"));
    assert_eq!(field_of(CAPTION, "other", "url"), Some("v"));
}

#[test]
fn a_trailing_comment_run_at_eof_ends_the_block() {
    let text = "pkg:\n  url: u\n# trailing note\n";
    let r = section_bounds(text, "pkg").unwrap();
    assert_eq!(spanned(&r, text), "pkg:\n  url: u\n");
}

/// The same comment, but the block's body resumes under it — so it is interior
/// and the block runs past it.
const INTERIOR: &str = "\
foo_pkg:
  url: https://example.invalid/foo
# a note
  tag: 1.0.0
bar_pkg:
  url: https://example.invalid/bar
";

#[test]
fn an_interior_comment_does_not_end_the_block() {
    let r = section_bounds(INTERIOR, "foo_pkg").unwrap();
    assert_eq!(
        spanned(&r, INTERIOR),
        "foo_pkg:\n  url: https://example.invalid/foo\n# a note\n  tag: 1.0.0\n"
    );
    assert_eq!(
        r.after(INTERIOR),
        "bar_pkg:\n  url: https://example.invalid/bar\n"
    );
}

#[test]
fn the_reader_sees_a_key_below_an_interior_comment() {
    // The write path replaces this `tag:`, so the read path must see it — if
    // the two disagreed, an upsert would leave a stale tag behind.
    assert_eq!(field_of(INTERIOR, "foo_pkg", "tag"), Some("1.0.0"));
    assert_eq!(field_of(INTERIOR, "bar_pkg", "tag"), None);
}

#[test]
fn blank_lines_inside_a_comment_run_do_not_split_it() {
    let text = "pkg:\n  url: u\n# a\n\n# b\n  tag: t\nother:\n";
    let r = section_bounds(text, "pkg").unwrap();
    assert_eq!(spanned(&r, text), "pkg:\n  url: u\n# a\n\n# b\n  tag: t\n");
    assert_eq!(field_of(text, "pkg", "tag"), Some("t"));
}

#[test]
fn field_of_reads_a_key_from_its_own_section_only() {
    let text = "package:\n  version: 1.0.0\nsource:\n  rev: abc\n  version: nope\n";
    assert_eq!(field_of(text, "package", "version"), Some("1.0.0"));
    assert_eq!(field_of(text, "source", "rev"), Some("abc"));
    assert_eq!(field_of(text, "build", "number"), None);
}

#[test]
fn field_of_stops_tracking_a_section_after_a_top_level_scalar() {
    // A top-level `key: value` closes the preceding block.
    let text = "source:\n  url: u\nschema_version: 1\n  rev: stray\n";
    assert_eq!(field_of(text, "source", "rev"), None);
}

// ---------------------------------------------------------------------------
// Line-ending and whitespace shapes
// ---------------------------------------------------------------------------

#[test]
fn line_ending_is_the_files_first_terminator() {
    assert_eq!(line_ending("a\r\nb\n"), "\r\n");
    assert_eq!(line_ending("a\nb\r\n"), "\n");
    assert_eq!(line_ending("no newline at all"), "\n");
    assert_eq!(line_ending(""), "\n");
}

#[test]
fn section_bounds_offsets_survive_crlf() {
    let text = "foo:\r\n  url: u\r\n\r\nbar:\r\n  url: v\r\n";
    let r = section_bounds(text, "foo").unwrap();
    // Byte offsets, so the CR is inside the span rather than lost.
    assert_eq!(spanned(&r, text), "foo:\r\n  url: u\r\n\r\n");
    assert_eq!(r.after(text), "bar:\r\n  url: v\r\n");
    assert_eq!(field_of(text, "bar", "url"), Some("v"));
}

#[test]
fn section_bounds_handles_a_file_with_no_trailing_newline() {
    let text = "foo:\n  url: u\nbar:\n  url: v";
    let r = section_bounds(text, "bar").unwrap();
    assert_eq!(spanned(&r, text), "bar:\n  url: v");
    assert_eq!(r.after(text), "");
    assert_eq!(field_of(text, "bar", "url"), Some("v"));
}

#[test]
fn tab_indented_content_stays_inside_its_block() {
    let text = "foo:\n\turl: u\n\ttag: 1.0.0\nbar:\n";
    let r = section_bounds(text, "foo").unwrap();
    assert_eq!(spanned(&r, text), "foo:\n\turl: u\n\ttag: 1.0.0\n");
    assert_eq!(field_of(text, "foo", "tag"), Some("1.0.0"));
}

#[test]
fn splicing_a_span_back_in_reproduces_the_file_byte_for_byte() {
    for text in [
        ROSDISTRO,
        INTERIOR,
        "foo:\r\n  url: u\r\nbar:\r\n  url: v\r\n",
        "foo:\n  url: u\nbar:\n  url: v",
        "foo:\n\turl: u\nbar:\n",
    ] {
        for key in ["foo", "bar", "foo_pkg", "bar_pkg"] {
            let Some(r) = section_bounds(text, key) else {
                continue;
            };
            let rebuilt = format!("{}{}{}", r.before(text), spanned(&r, text), r.after(text));
            assert_eq!(rebuilt, text, "key {key} in {text:?}");
        }
    }
}

const PIXI_NATIVE: &str = "\
rebuild_epoch: 0

packages:
  - name: alpha
    url: https://example.invalid/alpha
    rev: aaaa

  - name: beta
    url: https://example.invalid/beta
";

#[test]
fn item_bounds_stops_at_the_next_item_and_excludes_the_gap() {
    let lines: Vec<&str> = PIXI_NATIVE.lines().collect();
    let b = item_bounds(&lines, "alpha").unwrap();
    assert_eq!(*b.through_header(&lines).last().unwrap(), "  - name: alpha");
    assert_eq!(b.indent, 2);
    assert_eq!(b.sub_indent(), 4);
    // The blank separator line is inside `end` but outside `content_end`.
    assert_eq!(b.body(&lines), &lines[4..6]);
    assert_eq!(b.trailing(&lines)[0], "");
    assert_eq!(lines[b.end.0], "  - name: beta");
}

#[test]
fn item_bounds_runs_to_eof_for_the_last_item() {
    let lines: Vec<&str> = PIXI_NATIVE.lines().collect();
    let b = item_bounds(&lines, "beta").unwrap();
    assert_eq!(b.end.0, lines.len());
    assert_eq!(b.content_end.0, lines.len());
    assert!(b.trailing(&lines).is_empty());
}

#[test]
fn item_bounds_is_none_for_an_absent_name() {
    let lines: Vec<&str> = PIXI_NATIVE.lines().collect();
    assert!(item_bounds(&lines, "gamma").is_none());
    // A prefix of an existing name must not match.
    assert!(item_bounds(&lines, "alph").is_none());
}
