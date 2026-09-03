use super::*;

fn pkg(name: &str) -> PackageName {
    PackageName::new(name).unwrap()
}

#[test]
fn select_entries_filters_by_only_and_size() {
    let yaml = r"
packages:
  - name: alpha
    url: https://github.com/org/alpha
    rev: 1111111111111111111111111111111111111111
    runner-size: 4cpu
  - name: beta
    url: https://github.com/org/beta
    rev: 2222222222222222222222222222222222222222
    runner-size: 8cpu
  - name: gamma
    url: https://github.com/org/gamma
    rev: 3333333333333333333333333333333333333333
    runner-size: 4cpu
";
    let m = crate::types::PixiNativeManifest::from_yaml_str(yaml).unwrap();

    let sel = select_entries(&m.packages, None, &[pkg("alpha"), pkg("beta")]);
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);

    let sel = select_entries(
        &m.packages,
        Some("4cpu".parse().unwrap()),
        &[pkg("alpha"), pkg("beta")],
    );
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha"]);

    let sel = select_entries(&m.packages, Some("4cpu".parse().unwrap()), &[]);
    let names: Vec<&str> = sel.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "gamma"]);
}
