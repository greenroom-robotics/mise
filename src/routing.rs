//! Channel routing for published artifacts, mirroring ros-recipes'
//! `scripts/staging.py` (`load_routing_rules` / `resolve_channels`).
//!
//! ros-recipes' `routing.yaml` maps built .conda filenames to destination
//! channels via ordered glob rules — first match wins, no match routes to the
//! default channel. The buildfarm's already-published skip check must look in
//! the channel a package actually publishes to, not just `general`, or every
//! routed package (gama/lookout/missim cli+config, gama variants) rebuilds on
//! every run.

use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

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

/// Compile a routing.yaml glob pattern to an anchored regex. `*` becomes
/// `.*`; the literal token `{variant}` becomes a named non-greedy capture —
/// non-greedy so `gama_config_{variant}-*` against
/// `gama_config_austal_m_usv-5.0.0-py_0.conda` captures up to the FIRST `-`
/// (the version boundary). Same semantics as staging.py's _pattern_to_regex.
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
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            out.push_str(&regex::escape(&rest[..next]));
            rest = &rest[next..];
        }
    }
    out.push('$');
    regex::Regex::new(&out).with_context(|| format!("compile routing pattern {pattern}"))
}

/// Load `routing.yaml` from the repo root. A missing file yields no rules
/// (everything routes to the default channel — the pre-routing behaviour); a
/// malformed file errors loudly rather than silently mis-routing.
pub fn load_rules(repo_root: &Path) -> anyhow::Result<Vec<RoutingRule>> {
    let path = repo_root.join("routing.yaml");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let raw: RoutingYaml =
        serde_yaml_ng::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
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
/// A `{variant}` capture in the winning rule is substituted (underscores to
/// hyphens) into each of its channels. No match returns `None` — the caller
/// supplies the default channel.
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

/// The channel URLs the package `name` (at `version`) publishes to, derived
/// by swapping the last path segment of `default_channel_url` for each routed
/// channel name. Routing rules match built .conda filenames, so a synthetic
/// `name-version-0.conda` stands in — every rule is name-anchored. No routing
/// match means the package publishes to the default channel.
pub fn published_channel_urls(
    rules: &[RoutingRule],
    default_channel_url: &str,
    name: &str,
    version: &str,
) -> Vec<String> {
    let filename = format!("{name}-{version}-0.conda");
    let Some(channels) = resolve_channels(rules, &filename) else {
        return vec![default_channel_url.to_string()];
    };
    let base = default_channel_url.trim_end_matches('/');
    let base = base.rsplit_once('/').map(|(b, _)| b).unwrap_or(base);
    channels
        .into_iter()
        .map(|c| format!("{base}/{c}"))
        .collect()
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
