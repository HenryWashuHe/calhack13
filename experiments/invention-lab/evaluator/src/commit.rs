//! Hidden instances by commit/reveal: publish `commitment(salt, family)`
//! before development; reveal the salt only after methods are frozen.
use crate::design::hex;
use crate::family::{Family, FAMILY_VERSION};
use sha2::{Digest, Sha256};

pub fn commitment(salt: &str, family: Family) -> String {
    hex(&Sha256::digest(
        format!(
            "genesis-lab-hidden-v1:{salt}:{}:{FAMILY_VERSION}",
            family.name()
        )
        .as_bytes(),
    ))
}

/// The `index`-th hidden seed under a salt; always has the top bit set, so it
/// can't collide with a published development seed.
pub fn hidden_seed(salt: &str, family: Family, index: u32) -> u64 {
    let d = Sha256::digest(
        format!(
            "genesis-lab-seed-v1:{salt}:{}:{FAMILY_VERSION}:{index}",
            family.name()
        )
        .as_bytes(),
    );
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_le_bytes(b) | (1 << 63)
}
