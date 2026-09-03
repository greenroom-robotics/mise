//! Line-oriented block arithmetic for the vendored recipe YAML.
//!
//! The recipes repo's YAML is hand-maintained: comments, key order, blank-line
//! grouping and quoting style all carry intent, and a round-trip through a YAML
//! library rewrites all of it. So these files are edited by line surgery, and
//! this module is the shared definition of where a block *ends* — readers and
//! writers that disagreed about a block's extent would strand fields:
//!
//! * [`section_bounds`] / [`with_sections`] / [`field_of`] — a `<key>:` block
//!   at indent 0 (`rosdistro_additional_recipes.yaml`,
//!   `vendor_recipes/*/recipe.yaml`). All three read their answer off the
//!   single [`section_headers`] walk, so they cannot drift.
//! * [`item_bounds`] — a `- name: <name>` list item nested under `packages:`
//!   (`pixi_native_packages.yaml`), whose extent is settled by indentation
//!   relative to the `-`.
//!
//! # Where a top-level block ends
//!
//! At the next line in column 0 that is not a comment — a blank line never
//! ends a block, so the blank separating two entries belongs to the one above.
//!
//! A run of column-0 comments ends the block **unless the block's indented
//! body resumes after the run**: a flush-left comment is nearly always a
//! caption for the entry below it (`# fork of upstream` above `bar_pkg:`), and
//! absorbing it into the block above would mean an upsert silently deletes the
//! caption; but when indented content follows, the comment is interior, and
//! cutting the block short there would strand stale keys next to freshly
//! written ones.
//!
//! # Line endings
//!
//! [`section_bounds`] returns slices of the original text, so the splice
//! around it is byte-exact whatever the terminators are. The traversals
//! that rebuild a file line by line ([`with_sections`], [`item_bounds`]) hand
//! back lines stripped of their terminator; callers re-emit [`line_ending`],
//! which is the file's *first* terminator. A file with uniform endings
//! therefore round-trips exactly; a file that mixes CRLF and LF is normalized
//! to whichever it starts with. Nothing here preserves mixed endings.

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

pub fn indent_of(line: &str) -> usize {
    line.len().saturating_sub(line.trim_start().len())
}

/// Lines of `s` with their byte offsets, without line terminators.
fn lines_with_offsets(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;
    s.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset = offset.saturating_add(line.len());
        (start, line.trim_end_matches('\n').trim_end_matches('\r'))
    })
}

/// The terminator to re-emit when rebuilding `text` from its lines: the first
/// one the file uses, or LF for a file with no line break at all.
pub fn line_ending(text: &str) -> &'static str {
    match text.split_once('\n') {
        Some((before, _)) if before.ends_with('\r') => "\r\n",
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
    let mut rest = lines.iter().enumerate().peekable();
    while let Some((i, line)) = rest.next() {
        if is_indented(line) || is_blank(line) {
            out.push(current);
        } else if line.trim_start().starts_with('#') {
            // A column-0 comment run (blank lines inside it included) belongs
            // to the block above only if that block's body resumes after it.
            while rest
                .next_if(|(_, l)| {
                    !is_indented(l) && (is_blank(l) || l.trim_start().starts_with('#'))
                })
                .is_some()
            {}
            if !rest.peek().is_some_and(|(_, l)| is_indented(l)) {
                current = None;
            }
            let run_end = rest.peek().map_or(lines.len(), |(j, _)| *j);
            out.resize(run_end, current);
        } else {
            // Either a new `<key>:` header, or a top-level scalar that closes
            // whatever block preceded it.
            current = top_level_key(line).map(|_| i);
            out.push(current);
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
        .map(move |(line, header)| {
            let key = header
                .and_then(|h| lines.get(h))
                .and_then(|l| top_level_key(l));
            (key, line)
        })
}

/// A text split around one top-level `<key>:` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    before: &'a str,
    inner: &'a str,
    after: &'a str,
}

impl<'a> Section<'a> {
    pub const fn before(&self) -> &'a str {
        self.before
    }

    pub const fn after(&self) -> &'a str {
        self.after
    }
}

/// The top-level `<key>:` block named `key`, header line included.
///
/// See the module docs for where the block ends.
pub fn section_bounds<'a>(text: &'a str, key: &str) -> Option<Section<'a>> {
    let offsets: Vec<(usize, &str)> = lines_with_offsets(text).collect();
    let lines: Vec<&str> = offsets.iter().map(|(_, l)| *l).collect();
    let header = lines
        .iter()
        .position(|line| top_level_key(line) == Some(key))?;
    let (start, _) = *offsets.get(header)?;
    let end = section_headers(&lines)
        .iter()
        .zip(&offsets)
        .skip(header)
        .find(|(h, _)| **h != Some(header))
        .map_or(text.len(), |(_, (offset, _))| *offset);
    Some(Section {
        before: text.get(..start)?,
        inner: text.get(start..end)?,
        after: text.get(end..)?,
    })
}

/// The value of the `<key>:` line inside the top-level `<section>:` block, or
/// `None` if either is absent.
pub fn field_of<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    with_sections(text).find_map(|(cur, line)| {
        let trimmed = line.trim_start();
        (cur == Some(section) && indent_of(line) > 0)
            .then(|| trimmed.strip_prefix(&prefix))
            .flatten()
            .map(str::trim)
    })
}

/// The lines of a file split around one `- name: <name>` list item.
#[derive(Debug, PartialEq, Eq)]
pub struct ItemBounds<'a> {
    /// Column the `-` sits at; sub-keys sit two columns further in.
    pub indent: usize,
    through_header: &'a [&'a str],
    body: &'a [&'a str],
    trailing: &'a [&'a str],
}

impl<'a> ItemBounds<'a> {
    /// Indent of the item's sub-keys (`url:`, `rev:`, …).
    pub const fn sub_indent(&self) -> usize {
        self.indent.saturating_add(2)
    }

    /// Everything up to and including the `- name:` header line.
    pub const fn through_header(&self) -> &'a [&'a str] {
        self.through_header
    }

    /// The item's sub-key lines: after the header, up to its last non-blank line.
    pub const fn body(&self) -> &'a [&'a str] {
        self.body
    }

    /// The blank lines after the item plus the rest of the file.
    pub const fn trailing(&self) -> &'a [&'a str] {
        self.trailing
    }
}

/// Locate the `- name: <name>` item in `lines`.
///
/// The item ends at the first following non-blank line indented no further than
/// the `-` itself; blank lines inside the item do not end it.
pub fn item_bounds<'a>(lines: &'a [&'a str], name: &str) -> Option<ItemBounds<'a>> {
    let header_text = format!("- name: {name}");
    let (header, header_line) = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.trim_start() == header_text)?;
    let indent = indent_of(header_line);
    let after_header = header.checked_add(1)?;

    let end = lines
        .iter()
        .enumerate()
        .skip(after_header)
        .find(|(_, l)| !is_blank(l) && indent_of(l) <= indent)
        .map_or(lines.len(), |(i, _)| i);

    let content_end = (after_header..end)
        .rfind(|&i| lines.get(i).is_some_and(|l| !is_blank(l)))
        .map_or(after_header, |i| i.saturating_add(1));

    Some(ItemBounds {
        indent,
        through_header: lines.get(..after_header)?,
        body: lines.get(after_header..content_end)?,
        trailing: lines.get(content_end..)?,
    })
}

#[cfg(test)]
#[path = "yaml_block_tests.rs"]
mod tests;
