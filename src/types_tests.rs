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
        "app_config",
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
fn local_channel_is_an_empty_absolute_file_url() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("chan");
    std::fs::create_dir_all(dir.join("linux-64")).unwrap();
    let ch = LocalChannel::fresh(&dir).unwrap();
    let abs = std::fs::canonicalize(&dir).unwrap();
    assert_eq!(ch.to_string(), format!("file://{}/", abs.display()));
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}
