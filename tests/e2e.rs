//! End-to-end characterization suite: pins current behavior ahead of
//! refactoring.
//!
//! Runs the real `mise` binary in temp dirs against fixture trees, with
//! PATH-shim executables standing in for `gh`, `pixi` and `npx` (argv recorded,
//! canned stdout replayed) and real `git` operating on local temp repos only.
//! No network is touched anywhere. Golden files live under
//! `tests/e2e/fixtures/`; regenerate with `UPDATE_GOLDENS=1 cargo test`.

// A root test target resolves bare `mod` declarations against tests/, hence
// the explicit paths.
#[path = "e2e/harness.rs"]
mod harness;

#[path = "e2e/build_recipes_pixi.rs"]
mod build_recipes_pixi;
#[path = "e2e/matrix_compute.rs"]
mod matrix_compute;
#[path = "e2e/recipes_pr.rs"]
mod recipes_pr;
#[path = "e2e/release.rs"]
mod release;
