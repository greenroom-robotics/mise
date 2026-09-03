use std::path::Path;

use super::*;
use crate::types::{PackageName, RemoteChannel, Version};

fn rules_from(yaml: &str, dir: &Path) -> Vec<RoutingRule> {
    let file = dir.join(ROUTING_YAML);
    let routing = RoutingFile::Explicit {
        routing_file: file.clone(),
    };
    std::fs::write(file, yaml).unwrap();
    load_rules(routing).unwrap()
}

const SAMPLE: &str = r#"
rules:
  - pattern: app_config_{variant}-*
    channels: ["app-variant-{variant}"]
  - pattern: app_config-*
    channels: [app]
  - pattern: stream_config-*
    channels: [stream, general]
"#;

#[test]
fn missing_file_handled_correctly() {
    let td = tempfile::TempDir::new().unwrap();
    let file = td.path().join(ROUTING_YAML);

    // should fail
    let explicit = RoutingFile::Explicit { routing_file: file };
    assert!(load_rules(explicit).is_err());

    let default = RoutingFile::RepoDefault {
        repo_root: td.path().to_path_buf(),
    };
    let rules = load_rules(default);
    assert!(rules.is_ok());
    assert!(rules.unwrap().is_empty());
}

#[test]
fn first_match_wins_and_variant_substitutes() {
    let td = tempfile::TempDir::new().unwrap();
    let rules = rules_from(SAMPLE, td.path());
    // Non-greedy variant capture stops at the version boundary.
    assert_eq!(
        resolve_channels(&rules, "app_config_some_variant-5.0.0-py_0.conda"),
        Some(vec!["app-variant-some-variant".to_string()]),
    );
    // The `-` anchor keeps plain app_config out of the variant rule.
    assert_eq!(
        resolve_channels(&rules, "app_config-7.5.0-py_0.conda"),
        Some(vec!["app".to_string()]),
    );
    assert_eq!(
        resolve_channels(&rules, "stream_config-4.18.0-py_0.conda"),
        Some(vec!["stream".to_string(), "general".to_string()]),
    );
    assert_eq!(
        resolve_channels(&rules, "launch_utils-2.0.1-py_0.conda"),
        None
    );
}

#[test]
fn published_urls_swap_last_segment() {
    let td = tempfile::TempDir::new().unwrap();
    let rules = rules_from(SAMPLE, td.path());
    let base = RemoteChannel::parse("az://stg.blob.core.windows.net/general").unwrap();
    let chans = |name: &str, version: &str| -> Vec<String> {
        published_channels(
            &rules,
            &base,
            &PackageName::new(name).unwrap(),
            &Version::parse(version).unwrap(),
        )
        .iter()
        .map(std::string::ToString::to_string)
        .collect()
    };
    assert_eq!(
        chans("app_config", "7.5.0"),
        vec!["az://stg.blob.core.windows.net/app"],
    );
    assert_eq!(
        chans("app_config_some_boat", "7.5.0"),
        vec!["az://stg.blob.core.windows.net/app-variant-some-boat"],
    );
    assert_eq!(
        chans("stream_config", "4.18.0"),
        vec![
            "az://stg.blob.core.windows.net/stream",
            "az://stg.blob.core.windows.net/general",
        ],
    );
    // Unrouted packages keep the default channel URL untouched.
    assert_eq!(chans("launch_utils", "2.0.1"), vec![base.to_string()],);
}

#[test]
fn malformed_yaml_errors() {
    let td = tempfile::TempDir::new().unwrap();
    let routing = td.path().join(ROUTING_YAML);
    std::fs::write(&routing, "rules: notalist").unwrap();
    assert!(
        load_rules(RoutingFile::Explicit {
            routing_file: routing
        })
        .is_err()
    );
    assert!(
        load_rules(RoutingFile::RepoDefault {
            repo_root: td.path().to_path_buf()
        })
        .is_err()
    );
}
