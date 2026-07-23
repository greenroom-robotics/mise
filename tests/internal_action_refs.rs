// Composite actions can't use relative paths (`./`) for internal
// `greenroom-robotics/mise/...` refs when the action is consumed from an
// external repo — a relative `uses:` resolves against the *consumer's*
// checkout, not this repo's. So internal refs are hard-pinned to
// `@v<MAJOR>` instead, and every one of them must be bumped by hand on each
// major release. It's easy to bump the package version and the public
// `setup@vN` ref while forgetting one of the others, which silently leaves
// external consumers running a stale mise binary. This test is the
// tripwire: it fails if any internal ref's pinned major doesn't match
// pixi.toml's current major.
use regex::Regex;
use std::fs;
use std::path::Path;

#[test]
fn internal_action_refs_match_pixi_major() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let pixi_toml = fs::read_to_string(manifest_dir.join("pixi.toml")).unwrap();
    let pixi: toml::Value = pixi_toml.parse().unwrap();
    let version = pixi["package"]["version"].as_str().unwrap();
    let major = version.split('.').next().unwrap();

    let ref_re = Regex::new(r#"greenroom-robotics/mise/[^@\s"'#]+@v(\d+)"#).unwrap();

    let mut mismatches = Vec::new();
    for path in walk(&manifest_dir.join(".github")) {
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue, // skip non-utf8/binary files
        };
        for line in contents.lines() {
            for caps in ref_re.captures_iter(line) {
                let pinned_major = &caps[1];
                if pinned_major != major {
                    mismatches.push(format!(
                        "{}: {} (expected v{major})",
                        path.display(),
                        caps.get(0).unwrap().as_str()
                    ));
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "internal `greenroom-robotics/mise/...@vN` refs must be bumped to v{major} \
         (pixi.toml package.version is {version}):\n{}",
        mismatches.join("\n")
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
