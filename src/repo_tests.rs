use super::*;
use std::str::FromStr;
use tempfile::TempDir;

fn make_repo() -> (TempDir, Repo) {
    let td = TempDir::new().unwrap();
    fs::write(td.path().join("pixi.toml"), "[project]\n").unwrap();
    let repo = Repo::at(td.path()).unwrap();
    (td, repo)
}

#[test]
fn discover_walks_up() {
    let (td, _) = make_repo();
    let sub = td.path().join("a").join("b");
    fs::create_dir_all(&sub).unwrap();
    let found = Repo::discover_from(&sub).unwrap();
    assert_eq!(found.root(), td.path().canonicalize().unwrap());
}

#[test]
fn at_rejects_dir_without_pixi_toml() {
    let td = TempDir::new().unwrap();
    assert!(Repo::at(td.path()).is_err());
}

#[test]
fn deepstream_loader_parses_fixtures() {
    let (td, repo) = make_repo();
    let gh = td.path().join(".github");
    fs::create_dir_all(&gh).unwrap();
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/deepstream-recipes.yaml"
        ),
        gh.join("deepstream-recipes.yaml"),
    )
    .unwrap();
    let variants = td.path().join("variants");
    fs::create_dir_all(&variants).unwrap();
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/deepstream-variants.yaml"
        ),
        variants.join("deepstream.yaml"),
    )
    .unwrap();

    let cfg = repo.deepstream().unwrap();
    assert_eq!(cfg.recipes.len(), 2);
    assert_eq!(cfg.versions.len(), 2);
    assert!(
        cfg.recipes
            .contains(&RecipeName::from_str("deepstream-test1").unwrap())
    );
    assert!(
        cfg.recipes
            .contains(&RecipeName::from_str("deepstream-test2").unwrap())
    );
    assert!(cfg.versions.contains(&DeepstreamVersion::V7_1));
    assert!(cfg.versions.contains(&DeepstreamVersion::V8_0));
}

#[test]
fn pixi_native_loader_parses_valid_yaml() {
    let (td, repo) = make_repo();
    fs::write(
            td.path().join("pixi_native_packages.yaml"),
            "packages:\n  - name: foo\n    url: https://github.com/x/y.git\n    rev: 4110a9a40736b555c7419119ef6c607951563745\n",
        ).unwrap();
    let m = repo.pixi_native_manifest().unwrap();
    assert_eq!(m.packages.len(), 1);
}
