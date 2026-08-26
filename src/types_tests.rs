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
fn runner_spec_string_roundtrip() {
    for s in ["4cpu", "16cpu", "16cpu-himem", "4cpu-himem"] {
        let spec: RunnerSpec = s.parse().unwrap();
        assert_eq!(spec.to_string(), s);
    }
    assert_eq!(
        "16cpu-himem".parse::<RunnerSpec>().unwrap(),
        RunnerSpec {
            size: RunnerSize::Cpu16,
            himem: true
        }
    );
    assert!("himem".parse::<RunnerSpec>().is_err());
    assert!("2cpu-himem".parse::<RunnerSpec>().is_err());
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
    assert_eq!(u.owner(), "Greenroom-Robotics");
    assert_eq!(u.repo(), "mise");
}

#[test]
fn github_url_strips_dot_git() {
    let u = GithubRepoUrl::parse("https://github.com/foo/bar.git").unwrap();
    assert_eq!(u.repo(), "bar");
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
    assert_eq!(m.packages[0].name.as_str(), "pkg_with_rev");
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
fn entry_pin_style_defaults_to_range_and_parses_exact_pins() {
    let yaml = r#"
packages:
  - name: plain
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
  - name: lockstep
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
    exact-pins: true
"#;
    let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
    assert_eq!(m.packages[0].pin_style, SiblingPinStyle::Range);
    assert_eq!(m.packages[1].pin_style, SiblingPinStyle::Exact);
}

#[test]
fn entry_submodules_defaults_to_false_and_parses_when_set() {
    let yaml = r#"
packages:
  - name: plain
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
  - name: with_submodules
    url: https://github.com/example/repo.git
    rev: 4110a9a40736b555c7419119ef6c607951563745
    submodules: true
"#;
    let m = PixiNativeManifest::from_yaml_str(yaml).unwrap();
    assert!(!m.packages[0].submodules);
    assert!(m.packages[1].submodules);
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

#[test]
fn github_url_accepts_scp_style_ssh_remotes() {
    let u = GithubRepoUrl::parse_remote("git@github.com:foo/bar.git").unwrap();
    assert_eq!(u.slug(), "foo/bar");
    // The ssh form is normalised to the https form callers actually use.
    assert_eq!(u.https_url(), "https://github.com/foo/bar");
    assert_eq!(u.git_url(), "https://github.com/foo/bar.git");
}

#[test]
fn github_url_compare_link_has_no_git_suffix() {
    let u = GithubRepoUrl::parse_remote("https://github.com/foo/bar.git").unwrap();
    assert_eq!(
        u.compare_url("v1.0.0", "v1.1.0"),
        "https://github.com/foo/bar/compare/v1.0.0...v1.1.0"
    );
}

#[test]
fn package_name_accepts_the_shapes_the_manifests_carry() {
    for ok in [
        "mise",
        "gama_config",
        "ros-kilted-rclpy",
        "pixi-build-python",
        "Deepstream",
        "libx.y",
        "py3",
        // Real conda-forge packages and conda virtual packages: a manifest
        // that copies one of these in must still parse.
        "_libgcc_mutex",
        "_openmp_mutex",
        "_sysroot_linux-64_curr_repodata_hack",
        "__cuda",
        "__linux",
        "__glibc",
    ] {
        assert!(PackageName::new(ok).is_ok(), "should accept {ok}");
    }
}

#[test]
fn package_name_rejects_shapes_that_would_break_a_consumer() {
    // Empty, leading punctuation, whitespace, shell/path metacharacters and
    // the `pkg ==1.0` match-spec separators all have to be out.
    for bad in ["", "-lead", ".lead", "has space", "a/b", "a==1", "a*"] {
        assert!(PackageName::new(bad).is_err(), "should reject {bad:?}");
    }
}

#[test]
fn package_name_deserialises_through_its_constructor() {
    let err = serde_yaml_ng::from_str::<PackageName>("\"a b\"").unwrap_err();
    assert!(err.to_string().contains("package name"), "got: {err}");
}

#[test]
fn version_orders_numeric_prerelease_identifiers_numerically() {
    // The hand-rolled string sort this replaced put alpha.10 before alpha.2.
    assert!(Version::parse("1.0.0-alpha.2").unwrap() < Version::parse("1.0.0-alpha.10").unwrap());
    assert!(Version::parse("1.0.0-alpha.10").unwrap() < Version::parse("1.0.0").unwrap());
    assert!(Version::parse("1.9.0").unwrap() < Version::parse("1.21.0").unwrap());
}

#[test]
fn version_rejects_non_semver_conda_spellings() {
    for bad in [
        "",
        "rolling",
        "1!1.0.0",
        "2.5.*",
        "1.0.0.1",
        "1.0.0post1",
        "1.0.post1",
        "v1.2.3",
    ] {
        assert!(Version::parse(bad).is_err(), "should reject {bad}");
    }
}

// The lenient parse goes through `semver::Comparator`, which is a *requirement*
// parser. A range must not come back as the version it is anchored on.
#[test]
fn version_rejects_a_requirement_wearing_a_version_costume() {
    for req in [">=1.0", "<2", "~1.0", "^1.0", "=1.0", ">1.2.3"] {
        let err = Version::parse(req).unwrap_err();
        assert!(
            format!("{err:#}").contains("requirement"),
            "{req} should be refused as a requirement, got: {err:#}"
        );
    }
    // A wildcard starts with a digit, so it needs its own guard.
    for wild in ["2.5.*", "1.*", "*"] {
        assert!(Version::parse(wild).is_err(), "{wild} should be refused");
    }
}

// `Comparator` has no build-metadata field, so it would eat `+meta` in silence.
#[test]
fn version_rejects_build_metadata_rather_than_dropping_it() {
    let err = Version::parse("1.0.0+meta").unwrap_err();
    assert!(
        format!("{err:#}").contains("build metadata"),
        "got: {err:#}"
    );
}

#[test]
fn version_accepts_short_conda_spellings_and_reemits_them_verbatim() {
    // pixi/conda allow `1` and `1.0`; a manifest that says so must still say
    // so after we touch it.
    for (text, equivalent) in [("1", "1.0.0"), ("1.0", "1.0.0"), ("2.5", "2.5.0")] {
        let v = Version::parse(text).unwrap();
        assert_eq!(v.to_string(), text);
        assert_eq!(v, Version::parse(equivalent).unwrap());
    }
    // A prerelease still needs its patch component — `semver::Comparator`
    // only makes the numeric tail optional, not the separator before it.
    assert!(Version::parse("1.0-alpha.2").is_err());
    let v = Version::parse("1.0.0-alpha.2").unwrap();
    assert!(v < Version::parse("1.0.0").unwrap());
}

#[test]
fn version_padding_is_invisible_to_identity_but_not_to_display() {
    let short = Version::parse("1.0").unwrap();
    let long = Version::parse("1.0.0").unwrap();
    assert_eq!(short, long);
    assert_eq!(
        std::collections::BTreeSet::from([short.clone(), long.clone()]).len(),
        1
    );
    assert_ne!(short.to_string(), "1.0.0");
    // `1` fills out too.
    assert_eq!(Version::parse("1").unwrap(), long);
    // Only a spelled-out triple counts as an exact pin.
    assert!(!short.is_explicit_triple());
    assert!(
        Version::parse("1.0.0-alpha.1")
            .unwrap()
            .is_explicit_triple()
    );
}

#[test]
fn github_url_accepts_every_spelling_git_writes_for_a_remote() {
    for spelling in [
        "https://github.com/foo/bar.git",
        "git@github.com:foo/bar.git",
        "ssh://git@github.com/foo/bar.git",
        "ssh://github.com/foo/bar",
    ] {
        let u = GithubRepoUrl::parse_remote(spelling)
            .unwrap_or_else(|e| panic!("{spelling} should parse: {e:#}"));
        assert_eq!(u.slug(), "foo/bar");
    }
}

#[test]
fn version_range_pin_caps_at_the_next_major() {
    assert_eq!(Version::parse("0.3.1").unwrap().range_pin(), ">=0.3.1,<1");
    assert_eq!(
        Version::parse("1.24.0-alpha.2").unwrap().range_pin(),
        ">=1.24.0-alpha.2,<2"
    );
}

#[test]
fn sha40_from_str_matches_new() {
    let s = "4bcfd421c52387b3f7872b23e60059e521176f35";
    assert_eq!(s.parse::<Sha40>().unwrap(), Sha40::new(s).unwrap());
    assert!("nope".parse::<Sha40>().is_err());
}

#[test]
fn remote_channel_rejects_local_paths() {
    // A file: channel cannot be swept, so it must not be representable as a
    // RemoteChannel at all.
    assert!(RemoteChannel::parse("file:///tmp/out").is_err());
    assert!(RemoteChannel::parse("https://example.invalid/general").is_ok());
}

#[test]
fn remote_channel_sibling_swaps_the_last_segment() {
    let base = RemoteChannel::parse("az://stg.blob.core.windows.net/general").unwrap();
    assert_eq!(
        base.sibling("gama").to_string(),
        "az://stg.blob.core.windows.net/gama"
    );
    // A trailing slash is not a segment of its own.
    let slashed = RemoteChannel::parse("https://example.invalid/general/").unwrap();
    assert_eq!(
        slashed.sibling("gama").to_string(),
        "https://example.invalid/gama"
    );
}

#[test]
fn channel_url_round_trips_both_variants() {
    assert!(matches!(
        ChannelUrl::parse("file:///tmp/out").unwrap(),
        ChannelUrl::Local(_)
    ));
    assert_eq!(
        ChannelUrl::parse("file:///tmp/out").unwrap().to_string(),
        "file:///tmp/out"
    );
    assert!(matches!(
        ChannelUrl::parse("https://example.invalid/general").unwrap(),
        ChannelUrl::Remote(_)
    ));
}
