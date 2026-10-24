use genesis_lab_eval::design::{Body, BuildRules, Design, Joint, JointKind, Material};
use genesis_lab_eval::family::{instance, instance_digest, Family};
fn circles() -> Design {
    Design {
        language: 1,
        bodies: vec![
            Body::Circle {
                id: "a".into(),
                x: -4.0,
                y: 0.8,
                r: 0.75,
                material: Material::Wood,
            },
            Body::Circle {
                id: "b".into(),
                x: -2.8,
                y: 2.0,
                r: 0.75,
                material: Material::Wood,
            },
        ],
        joints: vec![],
        payload_on: "a".into(),
    }
}
fn rules() -> BuildRules {
    BuildRules {
        zone: (-6.0, 0.0, 0.0, 3.0),
        budget: 4.0,
    }
}
#[test]
fn separated_circles_inside_zone_and_budget_are_valid() {
    let d = circles();
    assert!((1.2_f32.powi(2) + 1.2_f32.powi(2)).sqrt() > 1.5);
    assert!(d.cost() < rules().budget);
    for b in &d.bodies {
        let (x0, y0, x1, y1) = b.aabb();
        assert!(x0 >= -6.0 && y0 >= 0.0 && x1 <= 0.0 && y1 <= 3.0);
    }
    assert!(d.validate(&rules()).is_ok(), "{:?}", d.validate(&rules()));
}
#[test]
fn anchor_outside_both_circles_is_rejected() {
    let mut d = circles();
    let (x, y) = (-3.4, 1.4);
    for b in &d.bodies {
        let (cx, cy) = b.center();
        assert!(((x - cx).powi(2) + (y - cy).powi(2)).sqrt() > 0.75 + 0.05);
    }
    d.joints.push(Joint {
        a: "a".into(),
        b: "b".into(),
        kind: JointKind::Hinge,
        x,
        y,
        motor_speed: None,
    });
    assert!(
        d.validate(&rules()).is_err(),
        "accepted physically off-body anchor: {:?}",
        d.validate(&rules())
    );
}
#[test]
fn separated_axis_aligned_control_and_round_trip_pass() {
    let mut d = circles();
    if let Body::Circle { x, .. } = &mut d.bodies[1] {
        *x = -2.0;
    }
    assert!(d.validate(&rules()).is_ok());
    let decoded: Design = serde_json::from_slice(&d.canonical()).unwrap();
    assert_eq!(d.canonical(), decoded.canonical());
    assert_eq!(d.digest(), decoded.digest());
}
#[test]
fn generated_world_bounds_and_exact_round_trips() {
    for f in Family::all() {
        for seed in (0..1000).chain([1_u64 << 63, u64::MAX]) {
            let w = instance(f, seed);
            assert_eq!(w.rules.zone, (-6.0, 0.0, 0.0, 3.0));
            assert!((4.0..=10.0).contains(&w.rules.budget));
            let parsed = serde_json::from_slice(&serde_json::to_vec(&w).unwrap()).unwrap();
            assert_eq!(w, parsed);
            assert_eq!(instance_digest(&w), instance_digest(&parsed));
            match f {
                Family::RavineCrossing | Family::ReturnMix => {
                    assert!((1.0..=2.5).contains(&w.gap));
                    assert!((-0.2..=0.2).contains(&w.step_height));
                    assert!((5.0..=30.0).contains(&w.payload_kg));
                }
                Family::RoughTransport => {
                    assert!((0.1..=0.35).contains(&w.bump_height));
                    assert!((0.6..=1.5).contains(&w.bump_spacing));
                    assert_eq!(w.distance, 8.0);
                    assert!((5.0..=30.0).contains(&w.payload_kg));
                }
                Family::ProjectileTarget => {
                    assert!((4.0..=9.0).contains(&w.target_x));
                    assert_eq!(w.target_half_width, 0.5);
                    assert!((1.0..=5.0).contains(&w.payload_kg));
                }
            }
            let (min, max) = if f == Family::ReturnMix {
                (0.3, 0.8)
            } else {
                (0.5, 1.0)
            };
            assert!((min..=max).contains(&w.friction));
            match f {
                Family::RoughTransport => assert!((1..=4).contains(&w.pushes.len())),
                Family::ReturnMix => assert!((1..=3).contains(&w.pushes.len())),
                _ => assert!(w.pushes.is_empty()),
            }
            for p in &w.pushes {
                assert!((120..=1800).contains(&p.step));
                assert!((-8.0..=8.0).contains(&p.dx));
                assert!((0.0..=5.0).contains(&p.dy));
            }
        }
    }
}
