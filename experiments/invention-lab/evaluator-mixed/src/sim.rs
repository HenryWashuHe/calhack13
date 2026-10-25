//! Deterministic 2D episode for one design on one `mixed-terrain` instance.
//! Body, joint, payload and stepping code is copied from v1's `sim.rs`
//! (unchanged there); only the terrain and the success rule differ. The copy
//! is hashed into this evaluator's identity.
use crate::family::{Instance, BUMPS_FROM, MAX_STEPS};
use genesis_lab_eval::design::{Body, Design, JointKind};
use genesis_lab_eval::family::STEPS_PER_SECOND;
use genesis_lab_eval::sim::{motor_joint, Episode, Frame, TRACE_EVERY};
use rapier2d::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const PAYLOAD_HALF: f32 = 0.2;

fn ground(
    bodies: &mut RigidBodySet,
    colliders: &mut ColliderSet,
    x0: f32,
    x1: f32,
    top: f32,
    friction: f32,
) {
    let hw = (x1 - x0) / 2.0;
    let h = bodies.insert(RigidBodyBuilder::fixed().translation(vector![x0 + hw, top - 0.5]));
    colliders.insert_with_parent(
        ColliderBuilder::cuboid(hw, 0.5).friction(friction),
        h,
        bodies,
    );
}

/// Runs one episode. A non-finite state ends it with `diverged`.
pub fn run(design: &Design, inst: &Instance) -> Episode {
    let mut bodies = RigidBodySet::new();
    let mut colliders = ColliderSet::new();
    let mut joints = ImpulseJointSet::new();
    let f = inst.friction;
    // Flat launch platform and near side up to the gap, with a bump field on it.
    ground(&mut bodies, &mut colliders, -20.0, inst.gap_start, 0.0, f);
    let mut x = BUMPS_FROM;
    while x < inst.bump_end {
        let h = bodies
            .insert(RigidBodyBuilder::fixed().translation(vector![x, inst.bump_height / 2.0]));
        colliders.insert_with_parent(
            ColliderBuilder::cuboid(0.1, inst.bump_height / 2.0).friction(f),
            h,
            &mut bodies,
        );
        x += inst.bump_spacing;
    }
    // Far bank.
    let far = inst.gap_start + inst.gap;
    ground(
        &mut bodies,
        &mut colliders,
        far,
        far + 20.0,
        inst.step_height,
        f,
    );
    let mut handle = BTreeMap::new();
    let mut order = vec![];
    for b in &design.bodies {
        let p = b.material().props();
        let (rb, col) = match b {
            Body::Rect {
                x, y, w, h, angle, ..
            } => (
                RigidBodyBuilder::dynamic()
                    .translation(vector![*x, *y])
                    .rotation(*angle),
                ColliderBuilder::cuboid(w / 2.0, h / 2.0),
            ),
            Body::Circle { x, y, r, .. } => (
                RigidBodyBuilder::dynamic().translation(vector![*x, *y]),
                ColliderBuilder::ball(*r),
            ),
        };
        let h = bodies.insert(rb);
        colliders.insert_with_parent(
            col.density(p.density)
                .friction(p.friction)
                .restitution(p.restitution),
            h,
            &mut bodies,
        );
        handle.insert(b.id().to_owned(), h);
        order.push(h);
    }
    let base = design
        .bodies
        .iter()
        .find(|b| b.id() == design.payload_on)
        .expect("validated payload_on");
    let (_, _, _, top) = base.aabb();
    let (cx, _) = base.center();
    let payload = bodies
        .insert(RigidBodyBuilder::dynamic().translation(vector![cx, top + PAYLOAD_HALF + 0.01]));
    colliders.insert_with_parent(
        ColliderBuilder::cuboid(PAYLOAD_HALF, PAYLOAD_HALF)
            .mass(inst.payload_kg)
            .friction(1.0),
        payload,
        &mut bodies,
    );
    order.push(payload);

    let mut live: Vec<(ImpulseJointHandle, f32)> = vec![];
    for j in &design.joints {
        let (ha, hb) = (handle[&j.a], handle[&j.b]);
        let pa = bodies[ha]
            .position()
            .inverse_transform_point(&point![j.x, j.y]);
        let pb = bodies[hb]
            .position()
            .inverse_transform_point(&point![j.x, j.y]);
        let joint: GenericJoint = match (j.kind, j.motor_speed) {
            // Both frames sit at the anchor with world orientation, so the
            // weld holds the bodies' initial relative pose, rotation included.
            (JointKind::Weld, _) => {
                let anchor = Isometry::translation(j.x, j.y);
                FixedJointBuilder::new()
                    .local_frame1(bodies[ha].position().inverse() * anchor)
                    .local_frame2(bodies[hb].position().inverse() * anchor)
                    .contacts_enabled(false)
                    .into()
            }
            (JointKind::Hinge, None) => RevoluteJointBuilder::new()
                .local_anchor1(pa)
                .local_anchor2(pb)
                .contacts_enabled(false)
                .into(),
            (JointKind::Hinge, Some(v)) => motor_joint(pa, pb, v),
        };
        let ma = design
            .bodies
            .iter()
            .find(|b| b.id() == j.a)
            .unwrap()
            .material();
        let mb = design
            .bodies
            .iter()
            .find(|b| b.id() == j.b)
            .unwrap()
            .material();
        let strength = ma.props().strength.min(mb.props().strength);
        live.push((joints.insert(ha, hb, joint, true), strength));
    }

    let mut pipeline = PhysicsPipeline::new();
    let mut islands = IslandManager::new();
    let mut broad = DefaultBroadPhase::new();
    let mut narrow = NarrowPhase::new();
    let mut multibody = MultibodyJointSet::new();
    let mut ccd = CCDSolver::new();
    let mut query = QueryPipeline::new();
    let params = IntegrationParameters {
        dt: 1.0 / STEPS_PER_SECOND as f32,
        ..Default::default()
    };
    let gravity = vector![0.0, -9.81];
    let mut hasher = Sha256::new();
    let mut frames = vec![];
    let mut broken = 0u32;
    let mut best_x = f32::MIN;
    let mut held = 0u32;
    let mut reached: Option<u32> = None;
    let mut failed = false;
    let mut diverged = false;
    let max = MAX_STEPS;
    let mut steps_run = 0;
    let (target, start_x) = (inst.target(), cx);
    for step in 0..max {
        for p in inst.pushes.iter().filter(|p| p.step == step) {
            bodies[payload].apply_impulse(vector![p.dx, p.dy], true);
        }
        pipeline.step(
            &gravity,
            &params,
            &mut islands,
            &mut broad,
            &mut narrow,
            &mut bodies,
            &mut colliders,
            &mut joints,
            &mut multibody,
            &mut ccd,
            Some(&mut query),
            &(),
            &(),
        );
        steps_run = step + 1;
        live.retain(|(jh, strength)| {
            let impulse = joints
                .get(*jh)
                .map(|j| j.impulses.xy().norm())
                .unwrap_or(0.0);
            if impulse > *strength * params.dt * 480.0 {
                joints.remove(*jh, true);
                broken += 1;
                false
            } else {
                true
            }
        });
        let p = *bodies[payload].translation();
        if !p.x.is_finite() || !p.y.is_finite() {
            diverged = true;
            break;
        }
        best_x = best_x.max(p.x);
        if step % TRACE_EVERY == 0 {
            let mut poses = Vec::with_capacity(order.len());
            for h in &order {
                let t = bodies[*h].position();
                let pose = [t.translation.x, t.translation.y, t.rotation.angle()];
                for v in pose {
                    hasher.update(v.to_bits().to_le_bytes());
                }
                poses.push(pose.map(|v| (v * 1000.0).round() / 1000.0));
            }
            frames.push(Frame { step, poses });
        }
        // Ravine's rule: lost below the gap, success after one second held
        // beyond the far edge plus one metre.
        if p.y < -1.5 {
            failed = true;
            break;
        }
        if p.x > target && p.y > inst.step_height - 0.5 {
            held += 1;
            reached.get_or_insert(step);
            if held >= STEPS_PER_SECOND {
                break;
            }
        } else {
            held = 0;
        }
    }
    let success = !diverged && !failed && held >= STEPS_PER_SECOND;
    let span = (target - start_x).max(0.001);
    let progress = if diverged {
        0.0
    } else {
        ((best_x - start_x) / span).clamp(0.0, 1.0)
    };
    Episode {
        success,
        progress: (progress * 1000.0).round() / 1000.0,
        joints_broken: broken,
        steps_run,
        steps_to_target: reached,
        trace_sha256: genesis_lab_eval::design::hex(&hasher.finalize()),
        frames,
        diverged,
    }
}
