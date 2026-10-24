//! Hidden instances by commit/reveal. The evaluator owner publishes
//! `commitment(comparison, salt, family, count)` before development, scores
//! privately with the salt, closes the comparison and only then reveals the
//! salt so anyone can regenerate the draw.
use crate::design::hex;
use crate::family::{Family, FAMILY_VERSION};
use sha2::{Digest, Sha256};

pub fn commitment(comparison: &str, salt: &str, family: Family, count: u32) -> String {
    hex(&Sha256::digest(
        format!(
            "genesis-lab-hidden-v1:{comparison}:{salt}:{}:{FAMILY_VERSION}:{count}",
            family.name()
        )
        .as_bytes(),
    ))
}

/// Seed `index` of the draw: first 8 bytes (little-endian) of the hash with
/// the top bit set, so it can't collide with a development seed (< 2^32).
pub fn hidden_seed(comparison: &str, salt: &str, family: Family, index: u32) -> u64 {
    let d = Sha256::digest(
        format!(
            "genesis-lab-seed-v1:{comparison}:{salt}:{}:{FAMILY_VERSION}:{index}",
            family.name()
        )
        .as_bytes(),
    );
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_le_bytes(b) | (1 << 63)
}
