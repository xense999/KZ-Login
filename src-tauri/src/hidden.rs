//! Key check for hidden features.
//!
//! The digest is compiled in, not the key itself: this repo is public, so a
//! plaintext key would sit in the git history forever even after being removed.
//! The digest is not real protection either — four characters fall to a trivial
//! brute force — it just keeps the key out of a `grep` of the source.

use sha2::{Digest, Sha256};

/// Not a secret. It only keeps the digest off public rainbow tables.
const HIDDEN_SALT: &str = "kz-login/hidden/v1";

/// (digest of salt + key, feature id). A second hidden feature is one more row.
const KEYS: &[(&str, &str)] = &[(
    "95ba010597ff68b5f715145bb3acfcfd06d27984e86070ee49dc5a7c082b374e",
    "export",
)];

/// The feature id a key unlocks, or `None` when nothing matches.
pub fn verify(input: &str) -> Option<&'static str> {
    let digest = Sha256::digest(format!("{HIDDEN_SALT}{}", input.trim()).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    KEYS.iter().find(|(h, _)| *h == hex).map(|(_, id)| *id)
}

