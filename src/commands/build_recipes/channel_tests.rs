use super::*;

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
    // A build we haven't published yet must still read as "needs building".
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 1, L64));
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.5"), 0, L64));
    // Dep satisfaction ignores the build number.
    assert!(idx.has_version(&pkg("autopilot"), &ver("3.5.4")));
    assert!(!idx.has_version(&pkg("autopilot"), &ver("3.5.5")));
    assert!(!idx.has_version(&pkg("geofence"), &ver("3.5.4")));
}

#[test]
fn channel_index_sees_noarch_packages() {
    // `pixi search -p linux-64` reports a noarch-only package under the
    // `noarch` key and omits `linux-64` entirely. Ignoring that key made
    // these look unpublished, so every noarch entry rebuilt and republished
    // on every run.
    let idx = index(
        r#"{"noarch":[
                 {"name":"gama_scenarios","version":"1.2.0","build_number":3,"subdir":"noarch"}
               ]}"#,
    );
    assert!(idx.has_build(&pkg("gama_scenarios"), &ver("1.2.0"), 3, NOARCH));
    assert!(idx.has_version(&pkg("gama_scenarios"), &ver("1.2.0")));
}

#[test]
fn channel_index_does_not_match_a_build_from_another_subdir() {
    // `vessel_offsets` 1.4.0 really does hold build 1 on linux-64 and build
    // 2 on noarch — a package that moved to noarch mid-version. Matching on
    // name+version+build alone would skip the noarch build 1 we still owe
    // (it only exists on linux-64) and skip the linux-64 build 2 as well.
    let idx = index(
        r#"{"linux-64":[
                 {"name":"vessel_offsets","version":"1.4.0","build_number":1,"subdir":"linux-64"}],
                "noarch":[
                 {"name":"vessel_offsets","version":"1.4.0","build_number":2,"subdir":"noarch"}]}"#,
    );
    assert!(idx.has_build(&pkg("vessel_offsets"), &ver("1.4.0"), 1, L64));
    assert!(idx.has_build(&pkg("vessel_offsets"), &ver("1.4.0"), 2, NOARCH));
    // The cross-subdir matches that must NOT skip a build.
    assert!(!idx.has_build(&pkg("vessel_offsets"), &ver("1.4.0"), 2, L64));
    assert!(!idx.has_build(&pkg("vessel_offsets"), &ver("1.4.0"), 1, NOARCH));
    // A sibling arch never satisfies another arch either.
    assert!(!idx.has_build(&pkg("vessel_offsets"), &ver("1.4.0"), 1, AARCH));
    // Dep satisfaction is still subdir-agnostic.
    assert!(idx.has_version(&pkg("vessel_offsets"), &ver("1.4.0")));
}

#[test]
fn build_subdir_follows_the_manifest_not_the_job_arch() {
    let noarch = PackageManifest::parse(
        "[package]\nname=\"p\"\nversion=\"1\"\n\
             [package.build.backend]\nname=\"pixi-build-python\"\nversion=\"*\"",
    )
    .unwrap();
    let arch = PackageManifest::parse("[package]\nname=\"x\"\nversion=\"1\"").unwrap();
    let l64 = Arch::Linux64;
    let a64 = Arch::LinuxAarch64;

    // A noarch package publishes to `noarch` whichever job builds it.
    assert_eq!(BuildSubdir::of(&noarch, l64), BuildSubdir::Noarch);
    assert_eq!(BuildSubdir::of(&noarch, a64), BuildSubdir::Noarch);
    // Everything else publishes to the job's own arch.
    assert_eq!(
        BuildSubdir::of(&arch, l64),
        BuildSubdir::Arch(Arch::Linux64)
    );
    assert_eq!(
        BuildSubdir::of(&arch, a64),
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
    // Sweep failure / empty channel must fail open into "needs building",
    // matching what the per-package searches did on error.
    let idx = ChannelIndex::from_records(&BTreeMap::new());
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 0, L64));
    assert!(!idx.has_build(&pkg("autopilot"), &ver("3.5.4"), 0, NOARCH));
    assert!(!idx.has_version(&pkg("autopilot"), &ver("3.5.4")));
}

// The publish check leans on this distinction: "channel says no such package"
// means build it, "channel could not be reached" means we have no idea and
// building would risk republishing.
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
