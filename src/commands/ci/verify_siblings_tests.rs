use super::*;
use crate::types::PackageName;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn latest_tagged_version_picks_semver_max_for_package() {
    let tags = vec![
        "geolocation@1.9.0".into(),
        "geolocation@1.21.0".into(),
        "geolocation@1.10.3".into(),
        "geolocation_node@9.9.9".into(),     // other package, ignored
        "geolocation@1.21.0-alpha.2".into(), // prerelease loses to release
    ];
    assert_eq!(
        latest_tagged_version(&tags, &PackageName::new("geolocation").unwrap()),
        Some("1.21.0")
    );
    assert_eq!(
        latest_tagged_version(&tags, &PackageName::new("nope").unwrap()),
        None
    );
}

// Prerelease identifiers that look numeric order numerically, so `alpha.10`
// is newer than `alpha.2`. Lexicographic string ordering gets this backwards.
#[test]
fn latest_tagged_version_orders_numeric_prereleases_numerically() {
    let tags = vec![
        "geolocation@1.0.0-alpha.2".into(),
        "geolocation@1.0.0-alpha.10".into(),
    ];
    assert_eq!(
        latest_tagged_version(&tags, &PackageName::new("geolocation").unwrap()),
        Some("1.0.0-alpha.10")
    );
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} failed");
}

/// repo with packages/dep (tagged dep@1.0.0) and packages/consumer with a
/// path dep on it. Returns (tempdir, consumer pixi.toml path).
fn fixture() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    git(&root, &["init", "-q", "-b", "main"]);
    for (name, extra) in [
        ("dep", String::new()),
        (
            "consumer",
            "[package.run-dependencies]\ndep = { path = \"../dep\" }\n".to_string(),
        ),
    ] {
        let dir = root.join("packages").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("pixi.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n{extra}"),
        )
        .unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "init"]);
    git(&root, &["tag", "dep@1.0.0"]);
    (tmp, root.join("packages/consumer/pixi.toml"))
}

#[test]
fn clean_sibling_at_tag_passes() {
    let (tmp, consumer) = fixture();
    let cmd = VerifySiblings {
        pixi_toml: consumer,
        package_dir: tmp.path().join("packages"),
    };
    assert!(cmd.run().is_ok());
}

#[test]
fn drifted_sibling_fails_with_remedy() {
    let (tmp, consumer) = fixture();
    let root = tmp.path();
    fs::write(root.join("packages/dep/src.py"), "changed").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "refactor: drift"]);
    let cmd = VerifySiblings {
        pixi_toml: consumer,
        package_dir: root.join("packages"),
    };
    let err = cmd.run().unwrap_err().to_string();
    assert!(err.contains("dep"), "names the sibling: {err}");
    assert!(err.contains("dep@1.0.0"), "names the tag: {err}");
}

#[test]
fn never_released_sibling_fails() {
    let (tmp, consumer) = fixture();
    let root = tmp.path();
    git(root, &["tag", "-d", "dep@1.0.0"]);
    let cmd = VerifySiblings {
        pixi_toml: consumer,
        package_dir: root.join("packages"),
    };
    let err = cmd.run().unwrap_err().to_string();
    assert!(err.contains("never been released") || err.contains("no release tag"));
}

// A tag we cannot order still means "released". Returning None here makes
// verify-siblings bail with "has never been released", which would block the
// release of a package whose only crime is an old tag spelling.
#[test]
fn latest_tagged_version_falls_back_to_an_unorderable_tag() {
    let tags = vec!["geolocation@1.0".into()];
    assert_eq!(
        latest_tagged_version(&tags, &PackageName::new("geolocation").unwrap()),
        Some("1.0")
    );
    let junk = vec!["geolocation@rolling".into()];
    assert_eq!(
        latest_tagged_version(&junk, &PackageName::new("geolocation").unwrap()),
        Some("rolling")
    );
    // ...but it never outranks a tag that does order.
    let mixed = vec!["geolocation@rolling".into(), "geolocation@0.1.0".into()];
    assert_eq!(
        latest_tagged_version(&mixed, &PackageName::new("geolocation").unwrap()),
        Some("0.1.0")
    );
}
