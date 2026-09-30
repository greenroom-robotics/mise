use super::*;
use std::fs;
use tempfile::TempDir;

/// Writes `packages/<dir>/pixi.toml` depending on each `(key, dir)` of `deps`
/// by path.
fn write_pkg(root: &Path, dir: &str, deps: &[(&str, &str)]) -> PathBuf {
    let pkg_dir = root.join("packages").join(dir);
    fs::create_dir_all(&pkg_dir).unwrap();
    let run_deps = deps
        .iter()
        .map(|(key, d)| format!("{key} = {{ path = \"../{d}\" }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let p = pkg_dir.join("pixi.toml");
    fs::write(
        &p,
        format!(
            "[workspace]\nname = \"{dir}\"\nchannels = [\"https://prefix.dev/conda-forge\"]\n\
             [dependencies]\n{dir} = {{ path = \".\" }}\n\
             [package]\nname = \"{dir}\"\nversion = \"1.2.0\"\n\
             [package.run-dependencies]\n{run_deps}\n"
        ),
    )
    .unwrap();
    p
}

fn channels_of(manifest: &Path) -> Vec<String> {
    let doc: toml::Value = toml::from_str(&fs::read_to_string(manifest).unwrap()).unwrap();
    doc["workspace"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn channel(root: &Path) -> LocalChannel {
    LocalChannel::fresh(&root.join("local-deps")).unwrap()
}

/// Prepares each of `top` in order, returning the package dirs published into
/// the channel, in publish order.
fn prepare_all(root: &Path, channel: LocalChannel, top: &[&str]) -> Result<Vec<String>> {
    let mut published = Vec::new();
    let mut deps = LocalDeps::new(channel, |m: &Path| {
        let dir = m.parent().unwrap().file_name().unwrap();
        published.push(dir.to_string_lossy().into_owned());
        Ok(())
    });
    for dir in top {
        let pkg = Package::read(&root.join("packages").join(dir).join("pixi.toml")).unwrap();
        deps.prepare(&pkg)?;
    }
    drop(deps);
    Ok(published)
}

#[test]
fn builds_nested_siblings_first_and_dedupes_a_diamond() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let lib = write_pkg(root, "lib", &[]);
    let a = write_pkg(root, "a", &[("ros-lib", "lib")]);
    let b = write_pkg(root, "b", &[("lib", "lib")]);
    let node = write_pkg(root, "node", &[("ros-a", "a"), ("b", "b")]);
    let ch = channel(root);

    assert_eq!(
        prepare_all(root, ch.clone(), &["node"]).unwrap(),
        ["lib", "a", "b"]
    );

    for consumer in [&node, &a, &b] {
        let channels = channels_of(consumer);
        assert_eq!(channels[0], ch.to_string());
        assert_eq!(channels.iter().filter(|c| **c == ch.to_string()).count(), 1);
    }
    assert_eq!(channels_of(&lib), ["https://prefix.dev/conda-forge"]);
    let text = fs::read_to_string(&node).unwrap();
    assert!(text.contains("ros-a = \">=1.2.0,<2\""), "rewritten: {text}");
}

#[test]
fn a_sibling_shared_by_two_packages_is_built_once() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_pkg(root, "lib", &[]);
    write_pkg(root, "x", &[("lib", "lib")]);
    write_pkg(root, "y", &[("ros-lib", "lib")]);

    assert_eq!(
        prepare_all(root, channel(root), &["x", "y"]).unwrap(),
        ["lib"]
    );
}

#[test]
fn a_package_prepared_earlier_can_still_be_built_as_a_sibling() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_pkg(root, "c", &[]);
    let b = write_pkg(root, "b", &[("c", "c")]);
    write_pkg(root, "a", &[("b", "b")]);
    let ch = channel(root);

    assert_eq!(
        prepare_all(root, ch.clone(), &["b", "a"]).unwrap(),
        ["c", "b"]
    );
    assert_eq!(channels_of(&b)[0], ch.to_string());
}

#[test]
fn a_package_without_path_deps_is_left_alone() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let solo = write_pkg(root, "solo", &[]);
    let before = fs::read_to_string(&solo).unwrap();

    assert!(
        prepare_all(root, channel(root), &["solo"])
            .unwrap()
            .is_empty()
    );
    assert_eq!(fs::read_to_string(&solo).unwrap(), before);
}

#[test]
fn a_manifest_rewritten_by_an_earlier_run_is_refused() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_pkg(root, "lib", &[]);
    write_pkg(root, "node", &[("lib", "lib")]);
    prepare_all(root, channel(root), &["node"]).unwrap();

    let msg = format!(
        "{:#}",
        prepare_all(root, channel(root), &["node"]).unwrap_err()
    );
    assert!(msg.contains("previous `mise ci build`"), "got: {msg}");
}

#[test]
fn a_cycle_among_siblings_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_pkg(root, "a", &[("ros-b", "b")]);
    write_pkg(root, "b", &[("ros-c", "c")]);
    write_pkg(root, "c", &[("b", "b")]);
    write_pkg(root, "node", &[("a", "a")]);

    let msg = format!(
        "{:#}",
        prepare_all(root, channel(root), &["node"]).unwrap_err()
    );
    assert!(
        msg.contains("path dep cycle: ros-b -> ros-c -> b"),
        "got: {msg}"
    );
}

#[test]
fn a_cycle_back_to_the_package_being_built_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_pkg(root, "node", &[("lib", "lib")]);
    write_pkg(root, "lib", &[("ros-node", "node")]);

    let msg = format!(
        "{:#}",
        prepare_all(root, channel(root), &["node"]).unwrap_err()
    );
    assert!(
        msg.contains("path dep cycle: node -> lib -> ros-node"),
        "got: {msg}"
    );
}
