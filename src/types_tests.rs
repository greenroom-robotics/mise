use super::*;

#[test]
fn arch_parses_known() {
    assert_eq!("linux-64".parse::<Arch>().unwrap(), Arch::Linux64);
    assert_eq!("linux-aarch64".parse::<Arch>().unwrap(), Arch::LinuxAarch64);
}

#[test]
fn arch_rejects_unknown() {
    assert!("osx-arm64".parse::<Arch>().is_err());
}

#[test]
fn deepstream_version_round_trips_via_serde() {
    let yaml = "\"7.1\"\n";
    let v: DeepstreamVersion = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(v, DeepstreamVersion::V7_1);
    let out = serde_yaml_ng::to_string(&DeepstreamVersion::V8_0).unwrap();
    assert_eq!(out.trim(), "'8.0'");
}

#[test]
fn runner_size_default_is_4cpu() {
    assert_eq!(RunnerSize::default(), RunnerSize::Cpu4);
}

#[test]
fn runner_size_serde() {
    let v: RunnerSize = serde_yaml_ng::from_str("16cpu").unwrap();
    assert_eq!(v, RunnerSize::Cpu16);
    assert!(serde_yaml_ng::from_str::<RunnerSize>("2cpu").is_err());
}

#[test]
fn deepstream_version_ord_is_ascending() {
    assert!(DeepstreamVersion::V7_1 < DeepstreamVersion::V8_0);
}

#[test]
fn sha40_accepts_valid() {
    let s = Sha40::new("4110a9a40736b555c7419119ef6c607951563745").unwrap();
    assert_eq!(s.as_str(), "4110a9a40736b555c7419119ef6c607951563745");
}

#[test]
fn sha40_rejects_uppercase() {
    assert!(Sha40::new("4110A9A40736b555c7419119ef6c607951563745").is_err());
}

#[test]
fn sha40_rejects_short() {
    assert!(Sha40::new("4110a9a40736").is_err());
}

#[test]
fn sha40_rejects_non_hex() {
    assert!(Sha40::new("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
}

#[test]
fn github_url_parses_https() {
    let u = GithubRepoUrl::parse("https://github.com/Greenroom-Robotics/mise").unwrap();
    assert_eq!(u.owner, "Greenroom-Robotics");
    assert_eq!(u.repo, "mise");
}

#[test]
fn github_url_strips_dot_git() {
    let u = GithubRepoUrl::parse("https://github.com/foo/bar.git").unwrap();
    assert_eq!(u.repo, "bar");
}

#[test]
fn github_url_rejects_non_github() {
    assert!(GithubRepoUrl::parse("https://gitlab.com/foo/bar").is_err());
}

#[test]
fn github_url_rejects_http() {
    assert!(GithubRepoUrl::parse("http://github.com/foo/bar").is_err());
}

#[test]
fn rejects_ref_entry_with_migration_pointer() {
    let yaml = r#"
packages:
  - name: pkg_with_ref
    url: https://github.com/example/repo.git
    ref: main
"#;
    let err = PixiNativeManifest::from_yaml_str(yaml).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("pkg_with_ref"), "got: {msg}");
    assert!(msg.contains("no longer supported"), "got: {msg}");
    assert!(msg.contains("ros-recipes/scripts"), "got: {msg}");
}

#[test]
fn rejects_missing_rev() {
    let yaml = r#"
packages:
  - name: pkg_without_rev
    url: https://github.com/example/repo.git
"#;
    let err = PixiNativeManifest::from_yaml_str(yaml).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("pkg_without_rev"), "got: {msg}");
    assert!(msg.contains("rev"), "got: {msg}");
}

#[test]
fn parses_valid_entries() {
    let yaml = r#"
packages:
  - name: pkg_with_rev
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
  - name: pkg_with_subdir
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
    subdir: packages/inner
    runner-size: 8cpu
"#;
    let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
    assert_eq!(m.packages.len(), 2);
    assert_eq!(m.packages[0].name, "pkg_with_rev");
    assert_eq!(
        m.packages[0].rev.as_str(),
        "4110a9a40736b555c7419119ef6c607951563745"
    );
    assert_eq!(m.packages[0].runner_size, RunnerSize::Cpu4);
    assert_eq!(
        m.packages[1].subdir.as_deref(),
        Some(std::path::Path::new("packages/inner"))
    );
    assert_eq!(m.packages[1].runner_size, RunnerSize::Cpu8);
}

#[test]
fn rebuild_epoch_defaults_to_zero() {
    let yaml = r#"
packages:
  - name: pkg
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
"#;
    let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
    assert_eq!(m.rebuild_epoch, 0);
}

#[test]
fn rebuild_epoch_parses_when_present() {
    let yaml = r#"
rebuild_epoch: 3
packages:
  - name: pkg
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
"#;
    let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
    assert_eq!(m.rebuild_epoch, 3);
}

#[test]
fn rebuild_epoch_rejects_negative() {
    let yaml = r#"
rebuild_epoch: -1
packages:
  - name: pkg
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
"#;
    assert!(PixiNativeManifest::from_yaml_str(yaml).is_err());
}

#[test]
fn rebuild_epoch_rejects_string() {
    let yaml = r#"
rebuild_epoch: "1"
packages:
  - name: pkg
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
"#;
    assert!(PixiNativeManifest::from_yaml_str(yaml).is_err());
}
