//! Hidden instances by commit/reveal (protocol `genesis-lab-hidden/2`).
//!
//! The evaluator owner publishes a [`Descriptor`] before development. It never
//! changes afterwards. The owner scores frozen designs privately with the salt,
//! closes the comparison and only then reveals the salt; the reveal is separate
//! evidence checked with [`Descriptor::opens`].
//!
//! Every field is length-prefixed (u64 big-endian length, then UTF-8 bytes;
//! integers as u64 big-endian), so `("a:b", "c")` and `("a", "b:c")` differ.
use crate::design::hex;
use crate::family::{Family, FAMILY_VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "genesis-lab-hidden/2";
const SEED_DOMAIN: &str = "genesis-lab-seed/2";

enum Field<'a> {
    Str(&'a str),
    Int(u64),
}

fn enc(fields: &[Field<'_>]) -> [u8; 32] {
    let mut h = Sha256::new();
    for f in fields {
        match f {
            Field::Str(s) => {
                h.update((s.len() as u64).to_be_bytes());
                h.update(s.as_bytes());
            }
            Field::Int(n) => h.update(n.to_be_bytes()),
        }
    }
    h.finalize().into()
}

/// The immutable reference a plan pins for its hidden set.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub protocol: String,
    pub comparison: String,
    pub family: Family,
    pub family_version: u32,
    pub count: u32,
    pub commitment: String,
}

fn commitment(comparison: &str, salt: &str, family: Family, version: u32, count: u32) -> String {
    hex(&enc(&[
        Field::Str(PROTOCOL),
        Field::Str(comparison),
        Field::Str(salt),
        Field::Str(family.name()),
        Field::Int(version as u64),
        Field::Int(count as u64),
    ]))
}

impl Descriptor {
    /// Made once by the salt holder; publish the result, keep the salt.
    pub fn new(comparison: &str, salt: &str, family: Family, count: u32) -> Self {
        Self {
            protocol: PROTOCOL.into(),
            comparison: comparison.into(),
            family,
            family_version: FAMILY_VERSION,
            count,
            commitment: commitment(comparison, salt, family, FAMILY_VERSION, count),
        }
    }

    /// Does this salt open the unchanged descriptor?
    pub fn opens(&self, salt: &str) -> bool {
        self.protocol == PROTOCOL
            && self.family_version == FAMILY_VERSION
            && commitment(
                &self.comparison,
                salt,
                self.family,
                self.family_version,
                self.count,
            ) == self.commitment
    }

    /// Seed `index` of the draw: first 8 bytes (little-endian) of the hash with
    /// the top bit set, so it can't collide with a development seed (< 2^32).
    /// Only meaningful for a salt that [`opens`](Self::opens) the descriptor.
    pub fn hidden_seed(&self, salt: &str, index: u32) -> u64 {
        let d = enc(&[
            Field::Str(SEED_DOMAIN),
            Field::Str(&self.comparison),
            Field::Str(salt),
            Field::Str(self.family.name()),
            Field::Int(self.family_version as u64),
            Field::Int(index as u64),
        ]);
        let mut b = [0u8; 8];
        b.copy_from_slice(&d[..8]);
        u64::from_le_bytes(b) | (1 << 63)
    }
}
