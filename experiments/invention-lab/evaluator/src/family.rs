//! Task families: a seeded generator plus a fixed success rule, versioned
//! together. An instance is `(family, version, seed)`.
use crate::design::BuildRules;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    RavineCrossing,
    RoughTransport,
    ReturnMix,
    ProjectileTarget,
}

pub const FAMILY_VERSION: u32 = 1;
/// Fixed step: 1/120 s.
pub const STEPS_PER_SECOND: u32 = 120;

impl Family {
    pub fn all() -> [Family; 4] {
        [
            Self::RavineCrossing,
            Self::RoughTransport,
            Self::ReturnMix,
            Self::ProjectileTarget,
        ]
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::RavineCrossing => "ravine-crossing",
            Self::RoughTransport => "rough-transport",
            Self::ReturnMix => "return-mix",
            Self::ProjectileTarget => "projectile-target",
        }
    }
    pub fn parse(s: &str) -> Option<Family> {
        Self::all().into_iter().find(|f| f.name() == s)
    }
    /// Episode length in steps; also what one experiment may consume at most.
    pub fn max_steps(self) -> u32 {
        match self {
            Self::ProjectileTarget => 10 * STEPS_PER_SECOND,
            _ => 20 * STEPS_PER_SECOND,
        }
    }
}

/// One horizontal or vertical push on the payload at a step.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Push {
    pub step: u32,
    pub dx: f32,
    pub dy: f32,
}

/// The concrete world an instance describes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub family: Family,
    pub version: u32,
    pub seed: u64,
    pub rules: BuildRules,
    pub payload_kg: f32,
    pub friction: f32,
    /// Ravine: gap width and far-bank height offset.
    pub gap: f32,
    pub step_height: f32,
    /// Rough track: bump height and spacing; transport distance.
    pub bump_height: f32,
    pub bump_spacing: f32,
    pub distance: f32,
    /// Projectile: target center and half width.
    pub target_x: f32,
    pub target_half_width: f32,
    pub pushes: Vec<Push>,
}

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
        // Rounded to millimetres so instances print and re-parse exactly.
        let v = lo as f64 + (hi as f64 - lo as f64) * self.next();
        ((v * 1000.0).round() / 1000.0) as f32
    }
    fn int(&mut self, lo: u32, hi: u32) -> u32 {
        lo + ((hi - lo + 1) as f64 * self.next()).floor() as u32
    }
}

pub fn instance(family: Family, seed: u64) -> Instance {
    // Different families never share a stream for the same seed.
    let mut r = Rng(seed ^ (family as u64 + 1).wrapping_mul(0xA24B_AED4_963E_E407));
    let mut i = Instance {
        family,
        version: FAMILY_VERSION,
        seed,
        rules: BuildRules {
            zone: (-6.0, 0.0, 0.0, 3.0),
            budget: 0.0,
        },
        payload_kg: 0.0,
        friction: 0.0,
        gap: 0.0,
        step_height: 0.0,
        bump_height: 0.0,
        bump_spacing: 0.0,
        distance: 0.0,
        target_x: 0.0,
        target_half_width: 0.0,
        pushes: vec![],
    };
    match family {
        Family::RavineCrossing | Family::ReturnMix => {
            i.gap = r.range(1.0, 2.5);
            i.step_height = r.range(-0.2, 0.2);
            i.payload_kg = r.range(5.0, 30.0);
            i.rules.budget = r.range(4.0, 10.0);
            i.friction = r.range(0.5, 1.0);
            if family == Family::ReturnMix {
                i.friction = r.range(0.3, 0.8);
                for _ in 0..r.int(1, 3) {
                    i.pushes.push(Push {
                        step: r.int(120, 1800),
                        dx: r.range(-8.0, 8.0),
                        dy: r.range(0.0, 5.0),
                    });
                }
            }
        }
        Family::RoughTransport => {
            i.bump_height = r.range(0.1, 0.35);
            i.bump_spacing = r.range(0.6, 1.5);
            i.distance = 8.0;
            i.payload_kg = r.range(5.0, 30.0);
            i.rules.budget = r.range(4.0, 10.0);
            i.friction = r.range(0.5, 1.0);
            for _ in 0..r.int(1, 4) {
                i.pushes.push(Push {
                    step: r.int(120, 1800),
                    dx: r.range(-8.0, 8.0),
                    dy: r.range(0.0, 5.0),
                });
            }
        }
        Family::ProjectileTarget => {
            i.target_x = r.range(4.0, 9.0);
            i.target_half_width = 0.5;
            i.payload_kg = r.range(1.0, 5.0);
            i.rules.budget = r.range(4.0, 10.0);
            i.friction = r.range(0.5, 1.0);
        }
    }
    i
}

/// Development seeds published per family: 0..count.
pub fn development_seeds(count: u64) -> Vec<u64> {
    (0..count).collect()
}

/// Identity of one instance: SHA-256 of its JSON (struct field order). Lets a
/// plan pin the exact world, not only `(family, version, seed)`.
pub fn instance_digest(inst: &Instance) -> String {
    use sha2::{Digest, Sha256};
    crate::design::hex(&Sha256::digest(
        serde_json::to_vec(inst).expect("instance serializes"),
    ))
}
