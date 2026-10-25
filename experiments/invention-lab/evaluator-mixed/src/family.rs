//! The `mixed-terrain` generator: a bump field, then a gap. Versioned
//! separately from v1; the ranges may change during calibration only and are
//! frozen (with this file's digest) before confirmation.
use genesis_lab_eval::design::BuildRules;
use genesis_lab_eval::family::{Push, STEPS_PER_SECOND};
use serde::{Deserialize, Serialize};

pub const FAMILY: &str = "mixed-terrain";
pub const FAMILY_VERSION: u32 = 1;
/// Episode length in steps (20 s), also the most one experiment may use.
pub const MAX_STEPS: u32 = 20 * STEPS_PER_SECOND;
/// Bumps begin here; the launch platform is flat left of x = 0.
pub const BUMPS_FROM: f32 = 0.5;
/// Flat run-up between the last bump position and the gap's near edge.
pub const RUN_UP: f32 = 0.5;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub family: String,
    pub version: u32,
    pub seed: u64,
    pub rules: BuildRules,
    pub payload_kg: f32,
    pub friction: f32,
    pub bump_height: f32,
    pub bump_spacing: f32,
    /// Bumps occupy [BUMPS_FROM, bump_end).
    pub bump_end: f32,
    /// The gap runs from `gap_start` to `gap_start + gap`.
    pub gap_start: f32,
    pub gap: f32,
    /// Far bank height relative to the near side.
    pub step_height: f32,
    pub pushes: Vec<Push>,
}

impl Instance {
    /// Success needs the payload this far right, held for one second.
    pub fn target(&self) -> f32 {
        self.gap_start + self.gap + 1.0
    }
}

/// v1's generator stream, with its own family salt.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let v = lo as f64 + (hi as f64 - lo as f64) * self.next();
        ((v * 1000.0).round() / 1000.0) as f32
    }
    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        lo + ((hi - lo + 1) as f64 * self.next()).floor() as u32
    }
}

/// Distinct from every v1 family's salt (v1 uses family index + 1 in 1..=4).
const SALT: u64 = 0x6D69_7865_642D_7431; // "mixed-t1"

pub fn instance(seed: u64) -> Instance {
    let mut r = Rng(seed ^ SALT.wrapping_mul(0xA24B_AED4_963E_E407));
    let bump_end = r.range(2.5, 4.5);
    let bump_height = r.range(0.08, 0.3);
    let bump_spacing = r.range(0.6, 1.4);
    let gap = r.range(0.6, 1.8);
    let step_height = r.range(-0.15, 0.15);
    let payload_kg = r.range(5.0, 30.0);
    let budget = r.range(4.0, 10.0);
    let friction = r.range(0.4, 0.9);
    let mut pushes = vec![];
    for _ in 0..r.int(0, 2) {
        pushes.push(Push {
            step: r.int(120, 1800),
            dx: r.range(-8.0, 8.0),
            dy: r.range(0.0, 5.0),
        });
    }
    Instance {
        family: FAMILY.into(),
        version: FAMILY_VERSION,
        seed,
        rules: BuildRules {
            zone: (-6.0, 0.0, 0.0, 3.0),
            budget,
        },
        payload_kg,
        friction,
        bump_height,
        bump_spacing,
        bump_end,
        gap_start: bump_end + RUN_UP,
        gap,
        step_height,
        pushes,
    }
}

/// SHA-256 of the instance's JSON (struct field order), as in v1.
pub fn instance_digest(inst: &Instance) -> String {
    use sha2::{Digest, Sha256};
    genesis_lab_eval::design::hex(&Sha256::digest(
        serde_json::to_vec(inst).expect("instance serializes"),
    ))
}
