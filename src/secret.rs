//! Values that must never reach a log line.
//!
//! Two layers, because either one alone leaks:
//!
//! - [`Secret`] wraps [`secrecy::SecretString`], which supplies the type-level
//!   guarantees: no `Display` at all, no `Debug` that can reach the plaintext,
//!   and zeroize-on-drop so the plaintext doesn't linger in freed memory. The
//!   plaintext is only reachable through [`ExposeSecret::expose_secret`],
//!   which is greppable. This module adds the `Debug` *text*: [`Secret`]
//!   prints exactly [`REDACTED`] so that debug output and scrubbed output are
//!   the same string.
//! - A process-wide scrub registry (`register`/[`scrub`]), which the crate
//!   does *not* provide and can't: constructing a `Secret` registers its
//!   plaintext, and [`crate::process`] scrubs every command label it logs or
//!   puts in an error. That covers the case `expose_secret()` exists for: an
//!   argument that *legitimately* embeds the token, like a tokenized clone
//!   URL or a `git config url.https://x-access-token:<token>@…` key. Those
//!   are ordinary `String`s by the time they reach the subprocess, so
//!   redaction has to happen at the logger rather than at the type.

use std::fmt;
use std::sync::{OnceLock, RwLock};

use zeroize::Zeroizing;

pub use secrecy::ExposeSecret;
use secrecy::SecretString;

/// A credential.
///
/// No `Display` impl exists, so formatting one into output is a compile
/// error rather than a redacted string. `Debug` renders the [`REDACTED`]
/// marker and nothing else. `Clone` is safe despite that: the plaintext is registered
/// with the scrub registry once, at construction, so every clone is already
/// covered.
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// Wrap `value` and register it for scrubbing. An empty value is not
    /// registered — scrubbing the empty string would redact everything.
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            register(&value);
        }
        Self(SecretString::from(value))
    }
}

impl ExposeSecret<str> for Secret {
    /// The plaintext. Only for handing to a subprocess or an HTTP header —
    /// never for anything that is formatted into output.
    fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for Secret {
    /// Writes [`REDACTED`] verbatim — byte-identical to what [`scrub`] leaves
    /// behind, so a redacted value looks the same however it got redacted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// What [`scrub`] replaces a registered secret with in a command label or
/// error message, and the whole of [`Secret`]'s `Debug` output.
pub const REDACTED: &str = "[REDACTED]";

/// The registered plaintexts, held in [`Zeroizing`] so the copies this module
/// keeps are wiped when the process tears the registry down.
///
/// Entries are never removed. Deregistering on drop would be unsound with
/// `Secret: Clone`: the first copy dropped would unregister a plaintext its
/// live clones still hold, and every later log line carrying that token would
/// go out in the clear. Scrubbing a secret that no longer exists is harmless;
/// failing to scrub one that does is the bug this exists to prevent.
fn registry() -> &'static RwLock<Vec<Zeroizing<String>>> {
    static REGISTRY: OnceLock<RwLock<Vec<Zeroizing<String>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

fn register(value: &str) {
    // A poisoned registry must not take down the program: the worst case of
    // recovering the inner value is a duplicate entry, and failing to scrub
    // would be far worse than that.
    let mut guard = registry()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !guard.iter().any(|v| v.as_str() == value) {
        guard.push(Zeroizing::new(value.to_string()));
    }
}

/// Replace every registered secret in `text` with [`REDACTED`].
#[must_use]
pub fn scrub(text: &str) -> String {
    registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .fold(text.to_string(), |out, value| {
            if out.contains(value.as_str()) {
                out.replace(value.as_str(), REDACTED)
            } else {
                out
            }
        })
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
