//! Literals that more than one module has to agree on.
//!
//! A constant earns a place here only when it appears at two or more sites and
//! a divergence between them would be a bug; single-use literals stay where
//! they are used.

/// Default `owner/repo` of the conda recipes repository.
/// (`ci release --recipes-repo`, `ci recipes-pr --recipes-repo`.)
pub const RECIPES_REPO: &str = "greenroom-robotics/ros-recipes";

/// Branch every repo we automate treats as the trunk: the base of PRs we
/// open, the branch we shallow-clone, and the branch whose workflow runs the
/// publish-SHA lookup queries.
pub const DEFAULT_BRANCH: &str = "main";

/// Conventional name of the remote a checkout fetches from and pushes to.
pub const ORIGIN: &str = "origin";

/// Upstream ROS channel: `snapshot refresh` caches its repodata and the vinca
/// build solves against it.
pub const ROBOSTACK_CHANNEL: &str = "https://prefix.dev/robostack-kilted";

/// Pixi workspace/package manifest filename.
pub const PIXI_TOML: &str = "pixi.toml";

/// Manifest of packages built straight from their own pixi manifests. Read by
/// `repo`, diffed by `matrix`, upserted into by `recipes_upsert`.
pub const PIXI_NATIVE_PACKAGES_YAML: &str = "pixi_native_packages.yaml";

/// Channel routing rules for published pixi-native packages.
pub const ROUTING_YAML: &str = "routing.yaml";

/// Manifest of extra rosdistro-sourced recipes. Diffed by `matrix`, upserted
/// into by `recipes_upsert`.
pub const ROSDISTRO_RECIPES_YAML: &str = "rosdistro_additional_recipes.yaml";
