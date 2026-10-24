//! Design language v1: rectangles and circles from a closed material table,
//! weld/hinge joints, open-loop hinge motors, a payload placement, inside a
//! task's build zone and part budget. Methods emit this; they can't extend it.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const LANGUAGE_VERSION: u32 = 1;
pub const MAX_BODIES: usize = 32;
pub const MAX_JOINTS: usize = 64;
pub const MAX_MOTOR_SPEED: f32 = 10.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Wood,
    Steel,
    Rubber,
}

/// Density (kg/m²), friction, restitution, cost per m², joint break impulse
/// contribution (N·s). Part of the evaluator identity.
pub struct MaterialProps {
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub cost_per_m2: f32,
    pub strength: f32,
}

impl Material {
    pub fn props(self) -> MaterialProps {
        match self {
            Self::Wood => MaterialProps {
                density: 600.0,
                friction: 0.8,
                restitution: 0.1,
                cost_per_m2: 1.0,
                strength: 300.0,
            },
            Self::Steel => MaterialProps {
                density: 7800.0,
                friction: 0.6,
                restitution: 0.05,
                cost_per_m2: 6.0,
                strength: 2000.0,
            },
            Self::Rubber => MaterialProps {
                density: 1100.0,
                friction: 1.6,
                restitution: 0.4,
                cost_per_m2: 2.0,
                strength: 150.0,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "shape", rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    Rect {
        id: String,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        #[serde(default)]
        angle: f32,
        material: Material,
    },
    Circle {
        id: String,
        x: f32,
        y: f32,
        r: f32,
        material: Material,
    },
}

impl Body {
    pub fn id(&self) -> &str {
        match self {
            Self::Rect { id, .. } | Self::Circle { id, .. } => id,
        }
    }
    pub fn center(&self) -> (f32, f32) {
        match self {
            Self::Rect { x, y, .. } | Self::Circle { x, y, .. } => (*x, *y),
        }
    }
    pub fn material(&self) -> Material {
        match self {
            Self::Rect { material, .. } | Self::Circle { material, .. } => *material,
        }
    }
    pub fn area(&self) -> f32 {
        match self {
            Self::Rect { w, h, .. } => w * h,
            Self::Circle { r, .. } => std::f32::consts::PI * r * r,
        }
    }
    /// Axis-aligned bounds (min x, min y, max x, max y).
    pub fn aabb(&self) -> (f32, f32, f32, f32) {
        match self {
            Self::Rect {
                x, y, w, h, angle, ..
            } => {
                let (c, s) = (angle.cos().abs(), angle.sin().abs());
                let hx = (w * c + h * s) / 2.0;
                let hy = (w * s + h * c) / 2.0;
                (x - hx, y - hy, x + hx, y + hy)
            }
            Self::Circle { x, y, r, .. } => (x - r, y - r, x + r, y + r),
        }
    }
    /// A point in this body's frame (rectangles rotate by `angle`).
    fn local(&self, px: f32, py: f32) -> (f32, f32) {
        let (x, y) = self.center();
        let (dx, dy) = (px - x, py - y);
        match self {
            Self::Rect { angle, .. } => {
                let (s, c) = angle.sin_cos();
                (dx * c + dy * s, -dx * s + dy * c)
            }
            Self::Circle { .. } => (dx, dy),
        }
    }
    /// Signed distance from a point to the shape (negative inside).
    fn signed_distance(&self, px: f32, py: f32) -> f32 {
        let (lx, ly) = self.local(px, py);
        match self {
            Self::Circle { r, .. } => (lx * lx + ly * ly).sqrt() - r,
            Self::Rect { w, h, .. } => {
                let (qx, qy) = (lx.abs() - w / 2.0, ly.abs() - h / 2.0);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                outside + qx.max(qy).min(0.0)
            }
        }
    }
    /// Is the point on the shape, within `tol` of its boundary?
    pub fn contains(&self, px: f32, py: f32, tol: f32) -> bool {
        self.signed_distance(px, py) <= tol
    }
    /// Corners of a rectangle in world coordinates.
    fn corners(&self) -> Option<[(f32, f32); 4]> {
        let Self::Rect {
            x, y, w, h, angle, ..
        } = self
        else {
            return None;
        };
        let (s, c) = angle.sin_cos();
        let (hw, hh) = (w / 2.0, h / 2.0);
        Some(
            [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)]
                .map(|(u, v)| (x + u * c - v * s, y + u * s + v * c)),
        )
    }
    fn numbers(&self) -> Vec<f32> {
        match self {
            Self::Rect {
                x, y, w, h, angle, ..
            } => vec![*x, *y, *w, *h, *angle],
            Self::Circle { x, y, r, .. } => vec![*x, *y, *r],
        }
    }
    fn size_ok(&self) -> bool {
        let ok = |v: f32| (0.05..=4.0).contains(&v);
        match self {
            Self::Rect { w, h, .. } => ok(*w) && ok(*h),
            Self::Circle { r, .. } => ok(*r),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    Weld,
    Hinge,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    pub a: String,
    pub b: String,
    pub kind: JointKind,
    /// World-space anchor at the start of the episode.
    pub x: f32,
    pub y: f32,
    /// Open-loop target angular speed (rad/s) for a hinge motor.
    #[serde(default)]
    pub motor_speed: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub language: u32,
    pub bodies: Vec<Body>,
    #[serde(default)]
    pub joints: Vec<Joint>,
    /// The body the payload is placed on, centered above it.
    pub payload_on: String,
}

/// The rectangle every body center must start in, and the part budget.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct BuildRules {
    pub zone: (f32, f32, f32, f32),
    pub budget: f32,
}

impl Design {
    /// Canonical encoding: the parsed design re-serialized (sorted struct
    /// order, shortest float form). Its SHA-256 is the design digest.
    pub fn canonical(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("design serializes")
    }
    pub fn digest(&self) -> String {
        hex(&Sha256::digest(self.canonical()))
    }
    pub fn cost(&self) -> f32 {
        self.bodies
            .iter()
            .map(|b| b.area() * b.material().props().cost_per_m2)
            .sum()
    }

    /// Checks the design against the language and a task's build rules.
    /// Returns the part cost, or why the design is invalid.
    pub fn validate(&self, rules: &BuildRules) -> Result<f32, String> {
        if self.language != LANGUAGE_VERSION {
            return Err(format!(
                "design language {} is not supported (this evaluator runs {LANGUAGE_VERSION})",
                self.language
            ));
        }
        if self.bodies.is_empty() || self.bodies.len() > MAX_BODIES {
            return Err(format!("1 to {MAX_BODIES} bodies are allowed"));
        }
        if self.joints.len() > MAX_JOINTS {
            return Err(format!("at most {MAX_JOINTS} joints are allowed"));
        }
        let mut ids = BTreeSet::new();
        let (zx0, zy0, zx1, zy1) = rules.zone;
        for b in &self.bodies {
            let id = b.id();
            if id.is_empty()
                || id.len() > 32
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
            {
                return Err("body ids are 1-32 of [a-z0-9_-]".into());
            }
            if !ids.insert(id.to_owned()) {
                return Err(format!("duplicate body id {id}"));
            }
            if b.numbers().iter().any(|v| !v.is_finite()) {
                return Err(format!("{id} has a non-finite number"));
            }
            if !b.size_ok() {
                return Err(format!("{id} size must be 0.05-4.0 m"));
            }
            // The whole body, not just its center: a long part centered at the
            // zone edge would otherwise start out over the task.
            let (x0, y0, x1, y1) = b.aabb();
            if x0 < zx0 || x1 > zx1 || y0 < zy0 || y1 > zy1 {
                return Err(format!("{id} starts outside the build zone"));
            }
        }
        let joined: BTreeSet<(String, String)> = self
            .joints
            .iter()
            .flat_map(|j| [(j.a.clone(), j.b.clone()), (j.b.clone(), j.a.clone())])
            .collect();
        for (i, a) in self.bodies.iter().enumerate() {
            for b in &self.bodies[i + 1..] {
                if joined.contains(&(a.id().to_owned(), b.id().to_owned())) {
                    continue;
                }
                // Bounding boxes first, then the exact shapes.
                let (a0, a1, a2, a3) = a.aabb();
                let (b0, b1, b2, b3) = b.aabb();
                let ox = a2.min(b2) - a0.max(b0);
                let oy = a3.min(b3) - a1.max(b1);
                if ox > 0.01 && oy > 0.01 && penetrates(a, b, 0.01) {
                    return Err(format!("{} and {} overlap at the start", a.id(), b.id()));
                }
            }
        }
        for j in &self.joints {
            if j.a == j.b || !ids.contains(&j.a) || !ids.contains(&j.b) {
                return Err("joints connect two different existing bodies".into());
            }
            if !j.x.is_finite() || !j.y.is_finite() {
                return Err("joint anchors must be finite".into());
            }
            for id in [&j.a, &j.b] {
                let body = self.bodies.iter().find(|b| b.id() == id).unwrap();
                if !body.contains(j.x, j.y, 0.05) {
                    return Err(format!("joint anchor is not on {id}"));
                }
            }
            match (j.kind, j.motor_speed) {
                (JointKind::Weld, Some(_)) => return Err("only hinges can have motors".into()),
                (_, Some(v)) if !v.is_finite() || v.abs() > MAX_MOTOR_SPEED => {
                    return Err(format!(
                        "motor speed must be within ±{MAX_MOTOR_SPEED} rad/s"
                    ))
                }
                _ => {}
            }
        }
        if !ids.contains(&self.payload_on) {
            return Err("payload_on names no body".into());
        }
        let cost = self.cost();
        if cost > rules.budget {
            return Err(format!(
                "cost {cost:.3} is over the part budget {:.3}",
                rules.budget
            ));
        }
        Ok(cost)
    }
}

/// Do two bodies interpenetrate by more than `tol` at the start? Exact for
/// circles and rotated rectangles (separating axes), not bounding boxes.
pub fn penetrates(a: &Body, b: &Body, tol: f32) -> bool {
    match (a, b, a.corners(), b.corners()) {
        (Body::Circle { r: ra, .. }, Body::Circle { r: rb, .. }, _, _) => {
            let ((ax, ay), (bx, by)) = (a.center(), b.center());
            ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt() - ra - rb < -tol
        }
        (rect, Body::Circle { x, y, r, .. }, _, _) | (Body::Circle { x, y, r, .. }, rect, _, _) => {
            rect.signed_distance(*x, *y) - r < -tol
        }
        (_, _, Some(pa), Some(pb)) => {
            // Separating axes: the edge normals of both rectangles. They
            // interpenetrate only if every axis shows an overlap deeper than tol.
            let axes = |p: &[(f32, f32); 4]| {
                [
                    (p[1].0 - p[0].0, p[1].1 - p[0].1),
                    (p[3].0 - p[0].0, p[3].1 - p[0].1),
                ]
            };
            axes(&pa).into_iter().chain(axes(&pb)).all(|(ux, uy)| {
                let n = (ux * ux + uy * uy).sqrt();
                let (ux, uy) = (ux / n, uy / n);
                let span = |p: &[(f32, f32); 4]| {
                    p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (x, y)| {
                        let d = x * ux + y * uy;
                        (lo.min(d), hi.max(d))
                    })
                };
                let ((a0, a1), (b0, b1)) = (span(&pa), span(&pb));
                a1.min(b1) - a0.max(b0) > tol
            })
        }
        _ => unreachable!("every non-circle body is a rectangle"),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
