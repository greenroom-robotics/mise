use super::*;

#[test]
fn reads_noarch_generic_and_python() {
    assert_eq!(
        Recipe::parse("build:\n  noarch: generic\n")
            .unwrap()
            .noarch(),
        Some(RecipeNoarch::Generic)
    );
    assert_eq!(
        Recipe::parse("build:\n  noarch: python\n")
            .unwrap()
            .noarch(),
        Some(RecipeNoarch::Python)
    );
}

#[test]
fn absent_or_null_noarch_is_arch_specific() {
    assert_eq!(
        Recipe::parse("build:\n  number: 0\n").unwrap().noarch(),
        None
    );
    assert_eq!(
        Recipe::parse("build:\n  noarch: ~\n").unwrap().noarch(),
        None
    );
    assert_eq!(
        Recipe::parse("package:\n  name: proto\n").unwrap().noarch(),
        None
    );
}

#[test]
fn templated_noarch_is_rejected() {
    let msg = format!(
        "{:#}",
        Recipe::parse("build:\n  noarch: ${{ noarch_kind }}\n").unwrap_err()
    );
    assert!(msg.contains("noarch"), "got: {msg}");
}

#[test]
fn unknown_noarch_value_is_rejected() {
    assert!(Recipe::parse("build:\n  noarch: true\n").is_err());
    assert!(Recipe::parse("build:\n  noarch: rust\n").is_err());
}

#[test]
fn templated_sibling_fields_do_not_matter() {
    let recipe = Recipe::parse(
        "context:\n  version: ${{ load_from_file(\"pixi.toml\").package.version }}\n\
         package:\n  name: proto\n  version: ${{ version }}\n\
         build:\n  number: 0\n  noarch: generic\n  script: ${{ '$RECIPE_DIR/build.sh' }}\n",
    )
    .unwrap();
    assert_eq!(recipe.noarch(), Some(RecipeNoarch::Generic));
}
