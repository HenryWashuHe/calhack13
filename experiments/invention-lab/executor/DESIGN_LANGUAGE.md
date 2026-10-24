# Design language 1 (public)

A design is one JSON object:

```json
{"language": 1,
 "payload_on": "deck",
 "bodies": [
   {"shape": "rect", "id": "deck", "x": -2.0, "y": 0.6, "w": 3.6, "h": 0.3,
    "angle": 0.0, "material": "wood"},
   {"shape": "circle", "id": "w1", "x": -3.5, "y": 0.45, "r": 0.3, "material": "rubber"}
 ],
 "joints": [
   {"a": "deck", "b": "w1", "kind": "hinge", "x": -3.5, "y": 0.45, "motor_speed": -5.0}
 ]}
```

Units are metres, radians, seconds and kilograms. Unknown fields are refused.

## Bodies

- `rect`: center `x`, `y`, width `w`, height `h`, optional `angle` (default 0).
- `circle`: center `x`, `y`, radius `r`.
- `id`: 1 to 32 characters from `a-z 0-9 _ -`, unique.
- Every size (`w`, `h`, `r`) is between 0.05 and 4.0.
- 1 to 32 bodies, at most 64 joints.

## Materials (closed table)

| material | density (kg/m²) | friction | restitution | cost per m² | joint strength |
| --- | --- | --- | --- | --- | --- |
| `wood` | 600 | 0.8 | 0.1 | 1.0 | 300 |
| `steel` | 7800 | 0.6 | 0.05 | 6.0 | 2000 |
| `rubber` | 1100 | 1.6 | 0.4 | 2.0 | 150 |

Cost is area × cost per m², summed over bodies; it must not exceed the
instance's `rules.budget`.

## Joints

- `kind`: `weld` (holds the two bodies' starting relative pose) or `hinge`
  (free rotation about the anchor).
- `x`, `y`: the world anchor at the start; it must lie on both bodies (within
  0.05 m of each shape).
- `motor_speed` (hinges only, optional): open-loop target angular speed, at
  most 10 rad/s in magnitude; the motor's torque is capped at 2000 N·m. There
  are no sensors or controllers.
- A joint breaks when its impulse in one step exceeds the weaker material's
  strength × 4 (strength × step × 480); a broken joint stays broken.

## Validity (checked before simulating; an invalid design costs 0 steps)

- `language` is 1.
- Every body starts wholly inside `rules.zone` = `[x0, y0, x1, y1]`
  (for version-1 families: x in [−6, 0], y in [0, 3]).
- No two bodies that aren't joined to each other overlap by more than 0.01 m
  at the start (exact shapes, rotation included).
- `payload_on` names a body; the 0.4 × 0.4 m payload of the instance's mass is
  placed centered above that body's top edge with a 0.01 m gap.

## Task families (version 1)

The world in each `instances` reply carries the drawn parameters.

- `ravine-crossing`: near bank at y = 0 for x ≤ 0; far bank at y = `step_height`
  from x = `gap` (1.0 to 2.5 m). Success: the payload stays past x = gap + 1 and
  above y = step_height − 0.5 for 120 consecutive steps (1 s). Failure: the
  payload falls below y = −1.5. Episode: 2400 steps (20 s).
- `rough-transport`: flat ground with 0.2 m-wide bumps from x = 0.5 m
  (`bump_height`, `bump_spacing`); 1 to 4 `pushes` (impulses on the payload at
  given steps). Success: the payload reaches x = 8 m before its bottom touches
  the ground. Episode: 2400 steps.
- `projectile-target`: a 1 m wall at x = 1.5 m; land the payload with its
  center inside `target_x` ± 0.5 m and below y = 1 at step 1200.

`progress` in metrics is the payload's furthest x advance over the distance to
the target line, between 0 and 1. Simulated results say nothing about physical
machines.
