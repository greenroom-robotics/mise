use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn collect_reports_copies_junit_xml_namespaced_by_package() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    let results = pkg_dir.join("build/foo/test_results/foo");
    fs::create_dir_all(&results).unwrap();
    fs::write(results.join("foo.gtest.xml"), "<testsuite/>").unwrap();
    // Non-XML files under build/ must be ignored.
    fs::write(pkg_dir.join("build/foo/other.txt"), "x").unwrap();
    // CTest's native Test.xml is not JUnit and must be excluded.
    let ctest = pkg_dir.join("build/foo/Testing/20240101-0000");
    fs::create_dir_all(&ctest).unwrap();
    fs::write(ctest.join("Test.xml"), "<Site/>").unwrap();
    // The ROS package manifest is not JUnit and must be excluded.
    fs::write(pkg_dir.join("build/foo/package.xml"), "<package/>").unwrap();

    let report_dir = tmp.path().join("test-reports");
    let n = collect_reports(&pkg_dir, &report_dir, "tests").unwrap();

    assert_eq!(n, 1);
    let dest = report_dir.join("foo/tests/build/foo/test_results/foo/foo.gtest.xml");
    assert!(dest.exists(), "expected {} to exist", dest.display());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "<testsuite/>");
}

#[test]
fn collect_reports_namespaces_by_env_so_variants_dont_collide() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("packages/foo");
    let results = pkg_dir.join("build/foo/test_results/foo");
    fs::create_dir_all(&results).unwrap();
    let report_dir = tmp.path().join("test-reports");

    // Standalone Asio run writes its XML, then the Boost run overwrites the
    // same path in build/ — but each is collected under its own env dir.
    fs::write(results.join("foo.gtest.xml"), "<standalone/>").unwrap();
    collect_reports(&pkg_dir, &report_dir, "tests").unwrap();
    fs::write(results.join("foo.gtest.xml"), "<boost/>").unwrap();
    collect_reports(&pkg_dir, &report_dir, "tests-boost").unwrap();

    let standalone = report_dir.join("foo/tests/build/foo/test_results/foo/foo.gtest.xml");
    let boost = report_dir.join("foo/tests-boost/build/foo/test_results/foo/foo.gtest.xml");
    assert_eq!(fs::read_to_string(&standalone).unwrap(), "<standalone/>");
    assert_eq!(fs::read_to_string(&boost).unwrap(), "<boost/>");
}

#[test]
fn collect_reports_returns_zero_when_no_build_dir() {
    let tmp = TempDir::new().unwrap();
    let pkg_dir = tmp.path().join("packages/empty");
    fs::create_dir_all(&pkg_dir).unwrap();
    let report_dir = tmp.path().join("test-reports");
    let n = collect_reports(&pkg_dir, &report_dir, "tests").unwrap();
    assert_eq!(n, 0);
}

#[test]
fn parse_jobs_defaults_to_default_test() {
    let jobs = parse_jobs(&[]).unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].env, "default");
    assert_eq!(jobs[0].task, "test");
}

#[test]
fn parse_jobs_parses_env_task_pairs() {
    let raw = vec!["tests:test".to_string(), "lint:lint".to_string()];
    let jobs = parse_jobs(&raw).unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        (jobs[0].env.as_str(), jobs[0].task.as_str()),
        ("tests", "test")
    );
    assert_eq!(
        (jobs[1].env.as_str(), jobs[1].task.as_str()),
        ("lint", "lint")
    );
}

#[test]
fn parse_jobs_rejects_malformed_specs() {
    assert!(parse_jobs(&["noselector".to_string()]).is_err());
    assert!(parse_jobs(&[":test".to_string()]).is_err());
    assert!(parse_jobs(&["tests:".to_string()]).is_err());
}
