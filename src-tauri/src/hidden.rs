//! Key check for hidden features. The plaintext key is neither here nor in git:
//! `build.rs` hashes it at compile time and injects only the digest, which is
//! all this module ever compares against.

use sha2::{Digest, Sha256};

/// Kept in sync with `build.rs` by hand. Not a secret — it only keeps the digest
/// off public rainbow tables.
const HIDDEN_SALT: &str = "kz-login/hidden/v1";

/// (compile-time digest, feature id). A second hidden feature is one more env
/// var in build.rs plus one more row here.
const KEYS: &[(&str, &str)] = &[(env!("KZ_HIDDEN_HASH_EXPORT"), "export")];

/// The feature id a key unlocks, or `None` when nothing matches.
pub fn verify(input: &str) -> Option<&'static str> {
    let digest = Sha256::digest(format!("{HIDDEN_SALT}{}", input.trim()).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    KEYS.iter().find(|(h, _)| *h == hex).map(|(_, id)| *id)
}
