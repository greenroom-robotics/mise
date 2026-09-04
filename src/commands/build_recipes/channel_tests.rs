use super::*;
use crate::manifest::Noarch;
use crate::recipe::RecipeNoarch;

fn pkg(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

fn ver(v: &str) -> Version {
    Version::parse(v).unwrap()
}

fn index(json: &str) -> ChannelIndex {
    ChannelIndex::from_records(&serde_json::from_str(json).unwrap())
}

const L64: BuildSubdir = BuildSubdir::Arch(Arch::Linux64);
const AARCH: BuildSubdir = BuildSubdir::Arch(Arch::LinuxAarch64);
const NOARCH: BuildSubdir = BuildSubdir::Noarch;

#[test]
fn channel_index_matches_exact_build_and_any_version() {
    let idx = index(
        r#"{"linux-64":[
                 {"name":"autopilot","version":"3.5.4","build_number":0,"subdir":"linux-64"},
                 {"name":"autopilot","version":"3.5.4","build_number":2,"subdir":"linux-64"}
               ]}"#,
    );
    assert!(idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 0, L64));
    assert!(idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 2, L64));
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 1, L64));
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.5"), 0, L64));
    assert!(idx.has_version(&pkg("autopilot"), &ver("3.5.4")));
    assert!(!idx.has_version(&pkg("autopilot"), &ver("3.5.5")));
    assert!(!idx.has_version(&pkg("geofence"), &ver("3.5.4")));
}

#[test]
fn channel_index_sees_noarch_packages() {
    // `pixi search -p linux-64` reports a noarch-only package under the
    // `noarch` key and omits `linux-64` entirely.
    let idx = index(
        r#"{"noarch":[
                 {"name":"alpha_scenarios","version":"1.2.0","build_number":3,"subdir":"noarch"}
               ]}"#,
    );
    assert!(idx.has_build(&pkg("alpha_scenarios"), &ver("1.2.0"), 3, NOARCH));
    assert!(idx.has_version(&pkg("alpha_scenarios"), &ver("1.2.0")));
}

#[test]
fn channel_index_does_not_match_a_build_from_another_subdir() {
    // A package that moved to noarch mid-version has records in both subdirs;
    // matching on name+version+build alone would cross-match them.
    let idx = index(
        r#"{"linux-64":[
                 {"name":"beta_offsets","version":"1.4.0","build_number":1,"subdir":"linux-64"}],
                "noarch":[
                 {"name":"beta_offsets","version":"1.4.0","build_number":2,"subdir":"noarch"}]}"#,
    );
    assert!(idx.has_build(&pkg("beta_offsets"), &ver("1.4.0"), 1, L64));
    assert!(idx.has_build(&pkg("beta_offsets"), &ver("1.4.0"), 2, NOARCH));
    assert!(!idx.has_build(&pkg("beta_offsets"), &ver("1.4.0"), 2, L64));
    assert!(!idx.has_build(&pkg("beta_offsets"), &ver("1.4.0"), 1, NOARCH));
    assert!(!idx.has_build(&pkg("beta_offsets"), &ver("1.4.0"), 1, AARCH));
    assert!(idx.has_version(&pkg("beta_offsets"), &ver("1.4.0")));
}

#[test]
fn build_subdir_follows_the_package_not_the_job_arch() {
    let l64 = Arch::Linux64;
    let a64 = Arch::LinuxAarch64;

    for noarch in [
        Noarch::PythonBackend,
        Noarch::AmentPython,
        Noarch::Recipe(RecipeNoarch::Generic),
        Noarch::Recipe(RecipeNoarch::Python),
    ] {
        assert_eq!(BuildSubdir::of(Some(noarch), l64), BuildSubdir::Noarch);
        assert_eq!(BuildSubdir::of(Some(noarch), a64), BuildSubdir::Noarch);
    }
    assert_eq!(BuildSubdir::of(None, l64), BuildSubdir::Arch(Arch::Linux64));
    assert_eq!(
        BuildSubdir::of(None, a64),
        BuildSubdir::Arch(Arch::LinuxAarch64)
    );
    // Display must match the subdir keys `pixi search --json` returns.
    assert_eq!(BuildSubdir::Noarch.to_string(), "noarch");
    assert_eq!(
        BuildSubdir::Arch(Arch::LinuxAarch64).to_string(),
        "linux-aarch64"
    );
}

#[test]
fn channel_index_empty_channel_publishes_nothing() {
    let idx = ChannelIndex::from_records(&BTreeMap::new());
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 0, L64));
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 0, NOARCH));
    assert!(!idx.has_version(&pkg("autopilot"), &ver("3.5.4")));
}

#[test]
fn an_unreachable_channel_is_told_apart_from_an_empty_one() {
    for stderr in [
        "error sending request for url (https://prefix.dev/general/noarch/repodata.json)",
        "  × failed to fetch repodata",
        "dns error: failed to lookup address information",
        "HTTP status client error (403 Forbidden) for url",
        "the operation timed out",
    ] {
        assert!(channel_unreachable(stderr), "{stderr:?}");
    }
    for stderr in [
        "",
        "No packages found matching 'foo==1.2.3'",
        "  × could not find package foo",
    ] {
        assert!(!channel_unreachable(stderr), "{stderr:?}");
    }
}
