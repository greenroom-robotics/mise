//! Values that must never reach a log line.
//!
//! Two layers, because either one alone leaks:
//!
//! - [`Secret`] renders as `***` through `Display` and `Debug`, so a token
//!   cannot be formatted into a message by accident. The plaintext is only
//!   reachable through [`Secret::expose`], which is greppable.
//! - Constructing a `Secret` also registers its plaintext with [`scrub`], and
//!   [`crate::process`] scrubs every command label it logs or puts in an
//!   error. That covers the case `expose()` exists for: an argument that
//!   *legitimately* embeds the token, like a tokenized clone URL or a `git
//!   config url.https://x-access-token:<token>@…` key. Those are ordinary
//!   `String`s by the time they reach the subprocess, so redaction has to
//!   happen at the logger rather than at the type.

use std::fmt;
use std::sync::{OnceLock, RwLock};

/// A credential.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap `value` and register it for scrubbing. An empty value is not
    /// registered — scrubbing the empty string would redact everything.
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            register(&value);
        }
        Self(value)
    }

    /// The plaintext. Only for handing to a subprocess or an HTTP header —
    /// never for anything that is formatted into output.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}

/// What a redacted value renders as.
pub const REDACTED: &str = "***";

fn registry() -> &'static RwLock<Vec<String>> {
    static REGISTRY: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

fn register(value: &str) {
    // A poisoned registry must not take down the program: the worst case of
    // recovering the inner value is a duplicate entry, and failing to scrub
    // would be far worse than that.
    let mut guard = registry().write().unwrap_or_else(|e| e.into_inner());
    if !guard.iter().any(|v| v == value) {
        guard.push(value.to_string());
    }
}

/// Replace every registered secret in `text` with [`REDACTED`].
pub fn scrub(text: &str) -> String {
    let guard = registry().read().unwrap_or_else(|e| e.into_inner());
    if guard.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for value in guard.iter() {
        if out.contains(value.as_str()) {
            out = out.replace(value.as_str(), REDACTED);
        }
    }
    out
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
