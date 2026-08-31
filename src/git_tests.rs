use super::*;
use crate::consts::DEFAULT_BRANCH;

/// A temp repo with one empty commit and a deterministic identity.
fn temp_repo() -> tempfile::TempDir {
    let td = tempfile::TempDir::new().unwrap();
    process::run_in(td.path(), "git", &["init", "--quiet"]).unwrap();
    commit(td.path(), &["--allow-empty", "-m", "init"]);
    td
}

fn commit(repo: &Path, args: &[&str]) {
    let mut argv = vec![
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t.com",
        "commit",
        "--quiet",
    ];
    argv.extend_from_slice(args);
    process::run_in(repo, "git", &argv).unwrap();
}

#[test]
fn nothing_staged_detects_no_pending_index_changes() {
    let td = temp_repo();
    let repo = td.path();

    assert!(nothing_staged(repo).unwrap());

    // Staging a file identical to the committed one leaves nothing staged.
    std::fs::write(repo.join("recipe.yaml"), "same content\n").unwrap();
    process::run_in(repo, "git", &["add", "recipe.yaml"]).unwrap();
    commit(repo, &["-m", "add recipe"]);
    std::fs::write(repo.join("recipe.yaml"), "same content\n").unwrap();
    process::run_in(repo, "git", &["add", "recipe.yaml"]).unwrap();
    assert!(nothing_staged(repo).unwrap());

    // An untracked file must not be mistaken for something to commit.
    std::fs::write(repo.join("untracked.tmp"), "leftover\n").unwrap();
    assert!(nothing_staged(repo).unwrap());
    std::fs::remove_file(repo.join("untracked.tmp")).unwrap();

    std::fs::write(repo.join("recipe.yaml"), "different content\n").unwrap();
    process::run_in(repo, "git", &["add", "recipe.yaml"]).unwrap();
    assert!(!nothing_staged(repo).unwrap());
}

#[test]
fn is_clean_distinguishes_untouched_from_changed_paths() {
    let td = temp_repo();
    let repo = td.path();
    std::fs::create_dir_all(repo.join("a")).unwrap();
    std::fs::create_dir_all(repo.join("b")).unwrap();
    std::fs::write(repo.join("a/f"), "1\n").unwrap();
    std::fs::write(repo.join("b/f"), "1\n").unwrap();
    process::run_in(repo, "git", &["add", "-A"]).unwrap();
    commit(repo, &["-m", "base"]);
    process::run_in(repo, "git", &["tag", "base"]).unwrap();

    std::fs::write(repo.join("a/f"), "2\n").unwrap();
    process::run_in(repo, "git", &["add", "-A"]).unwrap();
    commit(repo, &["-m", "touch a"]);

    assert!(!is_clean(repo, "base", "HEAD", Path::new("a")).unwrap());
    assert!(is_clean(repo, "base", "HEAD", Path::new("b")).unwrap());
}

#[test]
fn tags_lists_every_tag() {
    let td = temp_repo();
    process::run_in(td.path(), "git", &["tag", "pkg@1.0.0"]).unwrap();
    process::run_in(td.path(), "git", &["tag", "pkg@1.1.0"]).unwrap();
    assert_eq!(tags(td.path()).unwrap(), ["pkg@1.0.0", "pkg@1.1.0"]);
}

#[test]
fn file_at_rev_is_none_for_a_path_absent_at_that_rev() {
    let td = temp_repo();
    let repo = td.path();
    std::fs::write(repo.join("kept"), "body\n").unwrap();
    process::run_in(repo, "git", &["add", "-A"]).unwrap();
    commit(repo, &["-m", "add kept"]);
    let head = Sha40::new(
        process::capture_in(repo, "git", &["rev-parse", "HEAD"])
            .unwrap()
            .trim(),
    )
    .unwrap();

    assert_eq!(
        file_at_rev(repo, &head, "kept").unwrap().as_deref(),
        Some("body\n")
    );
    assert_eq!(file_at_rev(repo, &head, "never-existed").unwrap(), None);
}

#[test]
fn changed_files_lists_the_range() {
    let td = temp_repo();
    let repo = td.path();
    let base = process::capture_in(repo, "git", &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(repo.join("one"), "1\n").unwrap();
    std::fs::write(repo.join("two"), "2\n").unwrap();
    process::run_in(repo, "git", &["add", "-A"]).unwrap();
    commit(repo, &["-m", "two files"]);

    let changed = changed_files(repo, &format!("{base}..HEAD")).unwrap();
    assert_eq!(changed, [PathBuf::from("one"), PathBuf::from("two")]);
}

// --- clone / fetch, all against local repos, no network ---------------------

/// A source repo with two commits on `main`, cloneable over `file://`.
/// `uploadpack.allowAnySHA1InWant` is set repo-locally so fetching a
/// non-tip SHA works the way it does against GitHub.
fn origin_repo() -> (tempfile::TempDir, String, Sha40, Sha40) {
    let td = tempfile::TempDir::new().unwrap();
    let p = td.path();
    process::run_in(p, "git", &["init", "--quiet", "-b", DEFAULT_BRANCH]).unwrap();
    process::run_in(
        p,
        "git",
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    )
    .unwrap();
    std::fs::write(p.join("a.txt"), "first\n").unwrap();
    process::run_in(p, "git", &["add", "a.txt"]).unwrap();
    commit(p, &["-m", "one"]);
    let first = head_sha(p);
    std::fs::write(p.join("a.txt"), "second\n").unwrap();
    process::run_in(p, "git", &["add", "a.txt"]).unwrap();
    commit(p, &["-m", "two"]);
    let second = head_sha(p);
    let url = format!("file://{}", p.display());
    (td, url, first, second)
}

fn head_sha(repo: &Path) -> Sha40 {
    let out = process::capture_in(repo, "git", &["rev-parse", "HEAD"]).unwrap();
    Sha40::new(out.trim()).unwrap()
}

#[test]
fn shallow_clone_checks_out_the_named_branch() {
    let (_origin, url, _first, second) = origin_repo();
    let dest = tempfile::TempDir::new().unwrap();
    let into = dest.path().join("checkout");
    shallow_clone(&url, DEFAULT_BRANCH, &into).unwrap();
    assert_eq!(head_sha(&into).as_str(), second.as_str());
    assert_eq!(
        std::fs::read_to_string(into.join("a.txt")).unwrap(),
        "second\n"
    );
}

#[test]
fn fetch_rev_lands_on_an_arbitrary_commit_and_creates_the_destination() {
    let (_origin, url, first, _second) = origin_repo();
    let dest = tempfile::TempDir::new().unwrap();
    // Nested path that does not exist yet — fetch_rev must create it.
    let into = dest.path().join("nested/workdir");
    fetch_rev(&into, &url, &first).unwrap();
    assert_eq!(head_sha(&into).as_str(), first.as_str());
    assert_eq!(
        std::fs::read_to_string(into.join("a.txt")).unwrap(),
        "first\n"
    );
}

#[test]
fn fetch_branch_advances_fetch_head_without_moving_the_checkout() {
    let (origin, url, _first, _second) = origin_repo();
    let dest = tempfile::TempDir::new().unwrap();
    let into = dest.path().join("checkout");
    shallow_clone(&url, DEFAULT_BRANCH, &into).unwrap();
    let cloned_at = head_sha(&into);

    std::fs::write(origin.path().join("a.txt"), "third\n").unwrap();
    process::run_in(origin.path(), "git", &["add", "a.txt"]).unwrap();
    commit(origin.path(), &["-m", "three"]);
    let third = head_sha(origin.path());

    fetch_branch(&into, DEFAULT_BRANCH).unwrap();
    assert_eq!(
        head_sha(&into).as_str(),
        cloned_at.as_str(),
        "checkout moved"
    );
    let fetched = process::capture_in(&into, "git", &["rev-parse", "FETCH_HEAD"]).unwrap();
    assert_eq!(fetched.trim(), third.as_str());
}

// --- introspection ----------------------------------------------------------

#[test]
fn toplevel_finds_the_repo_root_from_a_subdirectory() {
    let td = temp_repo();
    let sub = td.path().join("a/b");
    std::fs::create_dir_all(&sub).unwrap();
    // Compare canonicalized: on macOS the temp dir is a symlink.
    assert_eq!(
        toplevel(&sub).unwrap().canonicalize().unwrap(),
        td.path().canonicalize().unwrap()
    );
}

#[test]
fn remote_url_reads_the_configured_remote() {
    let td = temp_repo();
    process::run_in(
        td.path(),
        "git",
        &["remote", "add", ORIGIN, "git@github.com:owner/repo.git"],
    )
    .unwrap();
    assert_eq!(
        remote_url(td.path(), ORIGIN).unwrap().trim(),
        "git@github.com:owner/repo.git"
    );
    assert!(remote_url(td.path(), "nope").is_err());
}

// --- rev_exists / file_at_rev ----------------------------------------------

// A shallow CI checkout genuinely does not have the base commit. Reading a
// file at that rev must say so, not report the file as absent — the two lead
// to opposite decisions in the matrix computation.
#[test]
fn file_at_rev_distinguishes_a_missing_commit_from_a_missing_file() {
    let (_origin, url, first, _second) = origin_repo();
    let dest = tempfile::TempDir::new().unwrap();
    let into = dest.path().join("shallow");
    // Depth-1 clone of the tip: `first` exists upstream but was never fetched.
    shallow_clone(&url, DEFAULT_BRANCH, &into).unwrap();
    assert!(!rev_exists(&into, &first).unwrap());
    let err = format!("{:#}", file_at_rev(&into, &first, "a.txt").unwrap_err());
    assert!(err.contains("not present"), "{err}");
    assert!(err.contains("fetch-depth"), "{err}");

    let head = head_sha(&into);
    assert!(rev_exists(&into, &head).unwrap());
    assert_eq!(
        file_at_rev(&into, &head, "a.txt").unwrap().as_deref(),
        Some("second\n")
    );
    // Present commit, absent path: that really is None.
    assert_eq!(file_at_rev(&into, &head, "nope.txt").unwrap(), None);
}
