// Internal `greenroom-robotics/mise/...` refs are hard-pinned to `@v<MAJOR>`
// (a relative `uses:` resolves against the consumer's checkout) and must all
// be bumped by hand on each major release; a forgotten one silently leaves
// external consumers on a stale mise binary. This test fails if any pinned
// major doesn't match the major mise is *about to* release.
//
// "about to": pixi.toml is only bumped by the release job after merge, so on
// the very PR that needs the refs bumped it still holds the old major. The
// expected major is therefore pixi.toml's plus one whenever there's a
// breaking commit since the release tag pixi.toml's version names.
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn internal_action_refs_match_pixi_major() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let pixi_toml = fs::read_to_string(manifest_dir.join("pixi.toml")).unwrap();
    let pixi: toml::Value = pixi_toml.parse().unwrap();
    let version = pixi["package"]["version"].as_str().unwrap();
    let released_major: u32 = version.split('.').next().unwrap().parse().unwrap();
    let breaking = breaking_since(manifest_dir, &format!("mise@{version}"));
    let major = released_major + u32::from(breaking);

    let ref_re = Regex::new(r#"greenroom-robotics/mise/[^@\s"'#]+@v(\d+)"#).unwrap();

    let mismatches: Vec<String> = walk(&manifest_dir.join(".github"))
        .into_iter()
        .filter_map(|path| fs::read_to_string(&path).ok().map(|c| (path, c)))
        .flat_map(|(path, contents)| {
            ref_re
                .captures_iter(&contents)
                .filter(|caps| caps[1].parse::<u32>().unwrap() != major)
                .map(|caps| format!("{}: {} (expected v{major})", path.display(), &caps[0]))
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        mismatches.is_empty(),
        "internal `greenroom-robotics/mise/...@vN` refs must be bumped to v{major} \
         (pixi.toml package.version is {version}{}):\n{}",
        if breaking {
            ", plus a breaking commit since that release"
        } else {
            ""
        },
        mismatches.join("\n")
    );
}

/// Does any commit after `tag` declare a breaking change, per conventional
/// commits (a `!` before the `:`, or a `BREAKING CHANGE` footer)? These are
/// what make semantic-release bump the major.
///
/// Returns false if git can't answer (no tag, shallow clone, no git) — the test
/// then just checks against the released major, as it did before.
fn breaking_since(repo: &Path, tag: &str) -> bool {
    let log = |format: &str| {
        Command::new("git")
            .args(["-C"])
            .arg(repo)
            .args(["log", format, &format!("{tag}..HEAD")])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };

    let bang = Regex::new(r"(?m)^[a-zA-Z]+(\([^)]*\))?!:").unwrap();
    bang.is_match(&log("--format=%s")) || log("--format=%B").contains("BREAKING CHANGE")
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
