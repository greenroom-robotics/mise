use super::*;

fn rules_from(yaml: &str, dir: &Path) -> Vec<RoutingRule> {
    std::fs::write(dir.join("routing.yaml"), yaml).unwrap();
    load_rules(dir).unwrap()
}

const SAMPLE: &str = r#"
rules:
  - pattern: gama_config_{variant}-*
    channels: ["gama-variant-{variant}"]
  - pattern: gama_config-*
    channels: [gama]
  - pattern: greenstream_config-*
    channels: [greenstream, general]
"#;

#[test]
fn missing_file_means_no_rules() {
    let td = tempfile::TempDir::new().unwrap();
    assert!(load_rules(td.path()).unwrap().is_empty());
}

#[test]
fn first_match_wins_and_variant_substitutes() {
    let td = tempfile::TempDir::new().unwrap();
    let rules = rules_from(SAMPLE, td.path());
    // Non-greedy variant capture stops at the version boundary.
    assert_eq!(
        resolve_channels(&rules, "gama_config_austal_m_usv-5.0.0-py_0.conda"),
        Some(vec!["gama-variant-austal-m-usv".to_string()]),
    );
    // The `-` anchor keeps plain gama_config out of the variant rule.
    assert_eq!(
        resolve_channels(&rules, "gama_config-7.5.0-py_0.conda"),
        Some(vec!["gama".to_string()]),
    );
    assert_eq!(
        resolve_channels(&rules, "greenstream_config-4.18.0-py_0.conda"),
        Some(vec!["greenstream".to_string(), "general".to_string()]),
    );
    assert_eq!(
        resolve_channels(&rules, "launch_ext-2.0.1-py_0.conda"),
        None
    );
}

#[test]
fn published_urls_swap_last_segment() {
    let td = tempfile::TempDir::new().unwrap();
    let rules = rules_from(SAMPLE, td.path());
    let base = "az://stg.blob.core.windows.net/general";
    assert_eq!(
        published_channel_urls(&rules, base, "gama_config", "7.5.0"),
        vec!["az://stg.blob.core.windows.net/gama"],
    );
    assert_eq!(
        published_channel_urls(&rules, base, "gama_config_blue_boat", "7.5.0"),
        vec!["az://stg.blob.core.windows.net/gama-variant-blue-boat"],
    );
    assert_eq!(
        published_channel_urls(&rules, base, "greenstream_config", "4.18.0"),
        vec![
            "az://stg.blob.core.windows.net/greenstream",
            "az://stg.blob.core.windows.net/general",
        ],
    );
    // Unrouted packages keep the default channel URL untouched.
    assert_eq!(
        published_channel_urls(&rules, base, "launch_ext", "2.0.1"),
        vec![base.to_string()],
    );
}

#[test]
fn malformed_yaml_errors() {
    let td = tempfile::TempDir::new().unwrap();
    std::fs::write(td.path().join("routing.yaml"), "rules: notalist").unwrap();
    assert!(load_rules(td.path()).is_err());
}
