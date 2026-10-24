use genesis_lab_eval::design::Design;
use genesis_lab_eval::family::{instance, Family};
use genesis_lab_eval::sim::run;

#[test]
fn review_weld_preserves_the_validated_initial_relative_angle() {
    // Two bars meet at (-2.5, 2), above the ground, with no initial overlap
    // with the payload. A weld must preserve their right-angle connection.
    let design: Design = serde_json::from_value(serde_json::json!({
        "language": 1,
        "payload_on": "horizontal",
        "bodies": [
            {"shape":"rect", "id":"horizontal", "x":-3.0, "y":2.0,
             "w":1.0, "h":0.2, "angle":0.0, "material":"wood"},
            {"shape":"rect", "id":"vertical", "x":-2.5, "y":2.5,
             "w":1.0, "h":0.2, "angle":std::f32::consts::FRAC_PI_2,
             "material":"wood"}
        ],
        "joints": [{"a":"horizontal", "b":"vertical", "kind":"weld",
                    "x":-2.5, "y":2.0}]
    }))
    .unwrap();
    let world = instance(Family::RavineCrossing, 3);
    design.validate(&world.rules).unwrap();
    let episode = run(&design, &world);
    assert!(!episode.diverged);
    let first = &episode.frames[0];
    let relative_angle = first.poses[1][2] - first.poses[0][2];
    assert!(
        (relative_angle - std::f32::consts::FRAC_PI_2).abs() < 0.02,
        "a validated 90-degree weld changed to {relative_angle} radians in the first simulated step; frame={first:?}"
    );
}

#[test]
fn review_unrotated_weld_control_preserves_relative_angle() {
    let design: Design = serde_json::from_value(serde_json::json!({
        "language": 1,
        "payload_on": "left",
        "bodies": [
            {"shape":"rect", "id":"left", "x":-3.0, "y":2.0,
             "w":1.0, "h":0.2, "material":"wood"},
            {"shape":"rect", "id":"right", "x":-2.0, "y":2.0,
             "w":1.0, "h":0.2, "material":"wood"}
        ],
        "joints": [{"a":"left", "b":"right", "kind":"weld",
                    "x":-2.5, "y":2.0}]
    }))
    .unwrap();
    let world = instance(Family::RavineCrossing, 3);
    design.validate(&world.rules).unwrap();
    let episode = run(&design, &world);
    assert!(!episode.diverged);
    let first = &episode.frames[0];
    assert!((first.poses[1][2] - first.poses[0][2]).abs() < 0.02);
}
