//! Channel routing for published artifacts.
//!
//! `routing.yaml` maps built .conda filenames to destination channels via
//! ordered glob rules — first match wins, no match routes to the default
//! channel. The buildfarm's already-published skip check must look in the
//! channel a package actually publishes to, not just the default, or every
//! routed package rebuilds on every run.

use std::path::PathBuf;

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::consts::ROUTING_YAML;
use crate::types::{PackageName, RemoteChannel, Version};

#[derive(Debug)]
pub struct RoutingRule {
    regex: regex::Regex,
    channels: Vec<String>,
}

#[derive(Deserialize)]
struct RoutingYaml {
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
struct RawRule {
    pattern: String,
    channels: Vec<String>,
}

pub enum RoutingFile {
    RepoDefault { repo_root: PathBuf },
    Explicit { routing_file: PathBuf },
}

/// Compile a routing.yaml glob pattern to an anchored regex. `*` becomes
/// `.*`; the literal token `{variant}` becomes a named non-greedy capture
fn pattern_to_regex(pattern: &str) -> anyhow::Result<regex::Regex> {
    let mut out = String::from("^");
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("{variant}") {
            out.push_str("(?P<variant>.+?)");
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('*') {
            out.push_str(".*");
            rest = tail;
        } else {
            let next = rest
                .char_indices()
                .find(|(i, _)| rest[*i..].starts_with("{variant}") || rest[*i..].starts_with('*'))
                .map_or(rest.len(), |(i, _)| i);
            out.push_str(&regex::escape(&rest[..next]));
            rest = &rest[next..];
        }
    }
    out.push('$');
    regex::Regex::new(&out).with_context(|| format!("compile routing pattern {pattern}"))
}

/// Load routing rules from `routing`, if the `RepoDefault` is missing no rules are returned,
/// otherwise an error.
pub fn load_rules(routing: RoutingFile) -> anyhow::Result<Vec<RoutingRule>> {
    let (routing_file, missing_ok) = match routing {
        RoutingFile::RepoDefault { repo_root } => (repo_root.join(ROUTING_YAML), true),
        RoutingFile::Explicit { routing_file } => (routing_file, false),
    };

    if !routing_file.is_file() {
        if missing_ok {
            return Ok(Vec::new());
        }
        bail!("{} is not a file", routing_file.display());
    }

    let text = std::fs::read_to_string(&routing_file)
        .with_context(|| format!("read {}", routing_file.display()))?;
    let raw: RoutingYaml = serde_yaml_ng::from_str(&text)
        .with_context(|| format!("parse {}", routing_file.display()))?;
    let mut rules = Vec::with_capacity(raw.rules.len());
    for (i, r) in raw.rules.into_iter().enumerate() {
        if r.pattern.is_empty() {
            anyhow::bail!("routing.yaml rules[{i}].pattern must be a non-empty string");
        }
        if r.channels.is_empty() || r.channels.iter().any(String::is_empty) {
            anyhow::bail!("routing.yaml rules[{i}].channels must be non-empty strings");
        }
        rules.push(RoutingRule {
            regex: pattern_to_regex(&r.pattern)?,
            channels: r.channels,
        });
    }
    Ok(rules)
}

/// First-match-wins routing of a .conda filename to destination channel(s).
///
/// A `{variant}` capture in the winning rule is substituted (underscores to
/// hyphens) into each of its channels. No match returns `None` — the caller
/// supplies the default channel.
#[must_use]
pub fn resolve_channels(rules: &[RoutingRule], filename: &str) -> Option<Vec<String>> {
    for rule in rules {
        let Some(m) = rule.regex.captures(filename) else {
            continue;
        };
        if let Some(variant) = m.name("variant") {
            let variant = variant.as_str().replace('_', "-");
            return Some(
                rule.channels
                    .iter()
                    .map(|c| c.replace("{variant}", &variant))
                    .collect(),
            );
        }
        return Some(rule.channels.clone());
    }
    None
}

/// The channels the package `name` (at `version`) publishes to, each a sibling
/// of `default_channel` (see [`RemoteChannel::sibling`]). Routing rules match
/// built .conda filenames, so a synthetic `name-version-0.conda` stands in —
/// every rule is name-anchored. No routing match means the package publishes
/// to the default channel.
#[must_use]
pub fn published_channels(
    rules: &[RoutingRule],
    default_channel: &RemoteChannel,
    name: &PackageName,
    version: &Version,
) -> Vec<RemoteChannel> {
    let filename = format!("{name}-{version}-0.conda");
    let Some(channels) = resolve_channels(rules, &filename) else {
        return vec![default_channel.clone()];
    };
    channels
        .iter()
        .map(|c| default_channel.sibling(c))
        .collect()
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
