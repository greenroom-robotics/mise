//! Line-oriented block arithmetic for the vendored recipe YAML.
//!
//! The recipes repo's YAML is hand-maintained: comments, key order, blank-line
//! grouping and quoting style all carry intent, and a round-trip through a YAML
//! library rewrites all of it. So `recipes_upsert` edits these files by line
//! surgery, replacing only the lines it owns.
//!
//! What that costs is a shared definition of where a block *ends*. Each file
//! shape has a reader (what was pinned before?) and a writer (pin this
//! instead), and if the two disagree about the extent of a block the reader
//! reports a field the writer did not replace. This module is that definition,
//! for two independently-shaped blocks:
//!
//! * [`section_bounds`] / [`with_sections`] / [`field_of`] — a `<key>:` block
//!   at indent 0, used by `rosdistro_additional_recipes.yaml` and
//!   `vendor_recipes/*/recipe.yaml`. All three read their answer off the
//!   single [`section_headers`] walk, so reader and writer cannot drift.
//! * [`item_bounds`] — a `- name: <name>` list item nested under `packages:`,
//!   used by `pixi_native_packages.yaml`. Its extent is settled by indentation
//!   relative to the `-`, which is a different question and has one answer.
//!
//! # Where a top-level block ends
//!
//! At the next line in column 0 that is not a comment — a blank line never
//! ends a block, so the blank separating two entries belongs to the one above.
//!
//! A column-0 *comment* is the ambiguous case, and the rule is: a run of
//! column-0 comments ends the block **unless the block's indented body resumes
//! after the run**. In these files a comment flush against the left margin is
//! nearly always a caption for the entry below it (`# fork of upstream` above
//! `bar_pkg:`), and absorbing it into the block above would mean an upsert of
//! that block silently deletes someone's caption. But when indented content
//! follows the comment, the comment is plainly interior to the block, and
//! cutting the block short there would leave the writer splicing over only
//! half an entry — stale keys stranded next to the ones it just wrote.
//!
//! # Line endings
//!
//! [`section_bounds`] returns byte offsets into the original text, so the
//! splice around it is byte-exact whatever the terminators are. The traversals
//! that rebuild a file line by line ([`with_sections`], [`item_bounds`]) hand
//! back lines stripped of their terminator; callers re-emit [`line_ending`],
//! which is the file's *first* terminator. A file with uniform endings
//! therefore round-trips exactly; a file that mixes CRLF and LF is normalized
//! to whichever it starts with. Nothing here preserves mixed endings.

use std::ops::Range;

/// The key of a top-level (`indent == 0`) block header line, e.g. `source:`.
///
/// `None` for anything else — including a top-level `key: value` line, which
/// *ends* the preceding block rather than opening one, and which callers
/// tracking the current section must therefore treat as a reset.
fn top_level_key(line: &str) -> Option<&str> {
    if is_indented(line) {
        return None;
    }
    let trimmed = line.trim_end();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    trimmed.strip_suffix(':')
}

fn is_indented(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Lines of `s` with their byte offsets, without line terminators.
fn lines_with_offsets(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;
    s.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line.trim_end_matches('\n').trim_end_matches('\r'))
    })
}

/// The terminator to re-emit when rebuilding `text` from its lines: the first
/// one the file uses, or LF for a file with no line break at all.
pub fn line_ending(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if text[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// For each line of `lines`, the index of the top-level header line whose block
/// it belongs to — `None` outside any block.
///
/// The one place the end-of-block rule described in the module docs is
/// implemented; every other traversal reads its answer off this.
fn section_headers(lines: &[&str]) -> Vec<Option<usize>> {
    let mut out: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut current: Option<usize> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_indented(line) || is_blank(line) {
            out.push(current);
            i += 1;
        } else if line.trim_start().starts_with('#') {
            // A column-0 comment run (blank lines inside it included) belongs
            // to the block above only if that block's body resumes after it.
            let mut run_end = i;
            while run_end < lines.len()
                && !is_indented(lines[run_end])
                && (is_blank(lines[run_end]) || lines[run_end].trim_start().starts_with('#'))
            {
                run_end += 1;
            }
            if !lines.get(run_end).is_some_and(|l| is_indented(l)) {
                current = None;
            }
            out.resize(run_end, current);
            i = run_end;
        } else {
            // Either a new `<key>:` header, or a top-level scalar that closes
            // whatever block preceded it.
            current = top_level_key(line).map(|_| i);
            out.push(current);
            i += 1;
        }
    }
    out
}

/// Each line of `text` paired with the top-level section it falls inside.
///
/// The header line itself is reported as being inside its own section. Lines
/// come back without their terminator — see the module docs on line endings.
pub fn with_sections(text: &str) -> impl Iterator<Item = (Option<&str>, &str)> {
    let lines: Vec<&str> = text.lines().collect();
    let headers = section_headers(&lines);
    lines
        .clone()
        .into_iter()
        .zip(headers)
        .map(move |(line, header)| (header.and_then(|h| top_level_key(lines[h])), line))
}

/// A byte range into the text it was measured from.
///
/// Distinct from [`LineIndex`] so that a range of one cannot be used to slice
/// the other: this module hands out both, and they are not interchangeable.
/// The range stays private for the same reason — callers splice through the
/// accessors rather than re-deriving offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteSpan(Range<usize>);

impl ByteSpan {
    /// Everything before the span.
    pub fn before<'a>(&self, text: &'a str) -> &'a str {
        &text[..self.0.start]
    }

    /// Everything after the span.
    pub fn after<'a>(&self, text: &'a str) -> &'a str {
        &text[self.0.end..]
    }
}

/// Byte span of the top-level `<key>:` block named `key`, header line included.
///
/// See the module docs for where the block ends.
pub fn section_bounds(text: &str, key: &str) -> Option<ByteSpan> {
    let offsets: Vec<(usize, &str)> = lines_with_offsets(text).collect();
    let lines: Vec<&str> = offsets.iter().map(|(_, l)| *l).collect();
    let header = lines
        .iter()
        .position(|line| top_level_key(line) == Some(key))?;
    let headers = section_headers(&lines);
    let end = headers[header + 1..]
        .iter()
        .position(|h| *h != Some(header))
        .map(|p| offsets[header + 1 + p].0)
        .unwrap_or(text.len());
    Some(ByteSpan(offsets[header].0..end))
}

/// The value of the `<key>:` line inside the top-level `<section>:` block, or
/// `None` if either is absent.
pub fn field_of<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    with_sections(text).find_map(|(cur, line)| {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        (cur == Some(section) && indent > 0)
            .then(|| trimmed.strip_prefix(&prefix))
            .flatten()
            .map(str::trim)
    })
}

/// An index into a `&[&str]` of lines.
///
/// Distinct from [`ByteSpan`] so that a line index cannot be used to slice the
/// text those lines came from.
/// The index stays private: slicing goes through the [`ItemBounds`] accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineIndex(usize);

/// Line-index extents of a `- name: <name>` list item.
#[derive(Debug, PartialEq, Eq)]
pub struct ItemBounds {
    /// Index of the `- name: <name>` line.
    pub header: LineIndex,
    /// Column the `-` sits at; sub-keys sit two columns further in. A column
    /// count, neither a line index nor a byte offset.
    pub indent: usize,
    /// One past the item's last non-blank line — where a rewritten body ends.
    pub content_end: LineIndex,
    /// One past the item's last line including the blank lines that separate
    /// it from the next item, which belong to the gap, not to either item.
    pub end: LineIndex,
}

impl ItemBounds {
    /// Indent of the item's sub-keys (`url:`, `rev:`, …).
    pub fn sub_indent(&self) -> usize {
        self.indent + 2
    }

    /// Everything up to and including the `- name:` header line.
    pub fn through_header<'a>(&self, lines: &'a [&'a str]) -> &'a [&'a str] {
        &lines[..=self.header.0]
    }

    /// The item's sub-key lines: after the header, up to `content_end`.
    pub fn body<'a>(&self, lines: &'a [&'a str]) -> &'a [&'a str] {
        &lines[self.header.0 + 1..self.content_end.0]
    }

    /// The gap after the item plus the rest of the file — passed through
    /// untouched by any rewrite.
    pub fn trailing<'a>(&self, lines: &'a [&'a str]) -> &'a [&'a str] {
        &lines[self.content_end.0..]
    }
}

/// Locate the `- name: <name>` item in `lines`.
///
/// The item ends at the first following non-blank line indented no further than
/// the `-` itself; blank lines inside the item do not end it.
pub fn item_bounds(lines: &[&str], name: &str) -> Option<ItemBounds> {
    let header_text = format!("- name: {name}");
    let header = lines.iter().position(|l| l.trim_start() == header_text)?;
    let indent = lines[header].len() - lines[header].trim_start().len();

    let end = lines[header + 1..]
        .iter()
        .position(|l| !is_blank(l) && l.len() - l.trim_start().len() <= indent)
        .map(|p| header + 1 + p)
        .unwrap_or(lines.len());

    let mut content_end = end;
    while content_end > header + 1 && is_blank(lines[content_end - 1]) {
        content_end -= 1;
    }

    Some(ItemBounds {
        header: LineIndex(header),
        indent,
        content_end: LineIndex(content_end),
        end: LineIndex(end),
    })
}

#[cfg(test)]
#[path = "yaml_block_tests.rs"]
mod tests;
