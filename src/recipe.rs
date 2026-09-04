use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecipeNoarch {
    Generic,
    Python,
}

#[derive(Debug)]
pub struct Recipe {
    noarch: Option<RecipeNoarch>,
}

#[derive(Debug, Deserialize)]
struct RecipeRaw {
    #[serde(default)]
    build: Option<RecipeBuild>,
}

#[derive(Debug, Deserialize)]
struct RecipeBuild {
    #[serde(default)]
    noarch: Option<RecipeNoarch>,
}

impl Recipe {
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RecipeRaw = serde_yaml_ng::from_str(text).context("parse recipe.yaml")?;
        Ok(Self {
            noarch: raw.build.and_then(|b| b.noarch),
        })
    }

    #[must_use]
    pub const fn noarch(&self) -> Option<RecipeNoarch> {
        self.noarch
    }
}

#[cfg(test)]
#[path = "recipe_tests.rs"]
mod tests;
