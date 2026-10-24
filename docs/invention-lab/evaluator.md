# Invention lab: evaluator and task contract

**Status: contract proposal for issue #126 by Henry's core Claude. Not
implemented.** It defines the simulated world, the task families, how instances
are drawn and hidden, how designs are scored, what a development experiment
costs and how all of that is identified. The development-method artifact and
its execution receipts are Codex's #124 contract; local adoption is
[adoption.md](adoption.md). This document only references their types.

Nothing here is a result. Simulated success says nothing about a physical
machine.

## Roles and trust

| Party | May | May not |
| --- | --- | --- |
| Evaluator (this contract) | define families, draw instances, run the simulator, score, issue receipts | be changed, configured or called for scoring by a candidate |
| Development method (#124) | read development instances, request bounded simulation experiments, return candidate designs and a trace | read hidden instances or seeds, call `score`, change budgets, set its own success criterion |
| Receiving organization (adoption.md) | choose a suite of development instances and a frozen acceptance criterion, ask the evaluator to score | supply the evaluator to the method or reuse hidden instances for acceptance |
| Comparison runner (#124/#126) | draw held-out instances after methods are frozen, score every treatment with the same evaluator version | rerun against an exposed holdout until a favorable result appears |

The evaluator runs in its own process. A method talks to it only through the
experiment interface below, and every call is metered there, not by the method.

## Design language v1

Small on purpose: enough for a cargo crossing to fail in interesting ways, not
a CAD system.

- **Bodies:** rectangles and circles, 2D, with size, position, angle and a
  material.
- **Materials:** a closed table (e.g. `wood`, `steel`, `rubber`) with density,
  friction, restitution and cost per unit area. Methods pick from the table;
  they can't define materials.
- **Joints:** `weld` and `hinge` between two bodies at an anchor, each with a
  break force from the material pair. A broken joint stays broken.
- **Motors:** on a hinge, with a target speed and a torque ceiling from a
  closed table. No sensors, controllers or code in v1; motors run open-loop for
  the episode.
- **Build zone and budget:** every body starts inside the task's build zone;
  the design's total cost must not exceed the task's part budget.
- **Encoding:** canonical JSON with sorted keys and fixed decimal precision.
  The design digest is the SHA-256 of that encoding. A design that fails
  validation (overlap at start, out of zone, over budget, unknown material) is
  scored `invalid` without simulating.

The language has a version. A method declares which version it emits
(adoption step 6); the evaluator refuses designs in another version.

## Simulator

- Rigid-body 2D engine with a fixed step (proposed: Rapier 2D with its
  cross-platform determinism option, in a Rust crate under
  `experiments/invention-lab/evaluator/`). The engine and its version are part
  of the evaluator identity.
- Fixed timestep, fixed solver iterations, episode length set by the task.
- **Determinism:** the same evaluator version, design, instance and seed give
  the same trace digest on the supported platform. This covers simulation only.
  Model calls inside a method are not replayable; their receipts are #124's.
- Output per run: outcome, metrics, a downsampled trace (body poses at a fixed
  rate) for display, and the trace digest.

## Task families

A family is a parameterized generator plus a fixed success rule. An instance is
`(family_id, family_version, seed)`; the generator maps it to concrete terrain,
payload and disturbances. Families are versioned; changing a range or rule is a
new version.

### `ravine-crossing` (A's visible task)

- **World:** two flat banks separated by a gap. Parameters: gap width, bank
  height difference, payload mass and size, part budget, ground friction.
- **Success:** the payload's center ends on the far bank past the target line,
  stays there for one simulated second, and never falls below the gap floor.
- **Metrics:** success, furthest payload progress as a fraction of the
  crossing, design cost, joints broken, time to target.

### `rough-transport` (B's related task)

- **World:** a heightfield track with bumps, plus lateral disturbance impulses
  at seeded times. Parameters: bump height/spacing, impulse magnitude and
  count, payload mass, friction, part budget.
- **Success:** the payload travels the track distance without touching the
  ground, within the episode.
- Related to crossing (same parts, same payload physics), different failure
  modes (interacting disturbances). A crossing method may help, may not.

### `return-mix` (A's fresh return challenge)

- **World:** crossing geometry combined with disturbances and changed friction
  or payload support, drawn from a range predeclared before B's revision is
  frozen. Instances are hidden until the comparison runs.

### `projectile-target` (the non-reuse control)

- **World:** launch a payload from a fixed base to land in a target zone.
- Shares the design language but rewards a different structure. A crossing or
  transport method should usually be declined or fail its acceptance here;
  that decision is scored separately from task success (an organization that
  declines and then fails the task has not succeeded).

These are proposals for the first families. Exact ranges are fixed in the
family files before any comparison, and no family is tuned to make a planned
method revision win.

## Development versus hidden instances

- **Development instances** are seeds the evaluator publishes per family,
  with full parameters. Methods and acceptance suites use only these.
- **Hidden instances** come from a secret salt held by the evaluator owner.
  Before a comparison, the evaluator publishes `sha256(salt || family ||
  version)`; after methods are frozen it reveals the salt, and anyone can
  regenerate and check the instances. A method or organization never receives
  hidden parameters before scoring.
- A hidden set is used once per declared comparison. Re-tuning after seeing it
  needs a new salt and a new comparison.

## Interfaces

Two calls, both on the evaluator process. Field names are placeholders until
#124 fixes the method-side types.

- `experiment(design, instance_ref, budget_token) -> ExperimentResult`
  - For development only. `instance_ref` must be a development instance.
  - Returns outcome, metrics, trace digest and downsampled trace.
  - Debits one experiment and the episode's simulated seconds from the budget
    token; refuses when exhausted. The token is issued by the method executor
    from the receiving organization's budget policy and can't be enlarged by
    the method.
- `score(design, instance_ref, purpose) -> ScoreReceipt`
  - Callers: an organization's acceptance check, or the comparison runner.
    Never a method.
  - `purpose` is `acceptance` (development instances only) or `comparison`
    (hidden instances, only after the salt is revealed for that comparison).
  - Returns a receipt (below).

Unknown is a first-class answer: a crashed or timed-out simulation returns
`unknown`, never `failed`, and is not silently retried by the evaluator.

## Receipts

`ScoreReceipt` binds:

- evaluator id, version and digest (engine version, scoring code, family files)
- design-language version and design digest
- family id/version, seed, and for hidden instances the salt commitment
- outcome: `succeeded`, `failed`, `invalid`, `unknown`
- metrics and trace digest
- purpose and caller organization
- simulated seconds and wall time consumed

Receipts are immutable. The comparison runner and acceptance checks cite them
by digest. Model identity, method identity and installation id belong in #124's
execution receipt, which cites the score receipts it led to.

## Budget and cost accounting

Per development task, the receiving organization's policy sets:

- maximum experiments and simulated seconds (enforced here),
- wall-time and model-usage ceilings (enforced by the method executor, #124).

The comparison counts everything: cold method development, failed attempts and
experiments, not only the final design's run. Equal total budgets across
treatments are the primary comparison; per-task warm numbers are secondary.

## What this needs from other contracts

- **#124:** the method I/O types carrying `instance_ref`s and designs, the
  budget token issued by the executor, and an execution receipt that cites
  score receipts.
- **adoption.md:** acceptance suites are lists of development `instance_ref`s
  plus a criterion over `ScoreReceipt` outcomes; the evaluator requirement a
  method declares is `evaluator_id@version`.
- **Evidence UI (#123):** the downsampled trace for animation, score receipts
  for the outcome panel, and the distinction `failed` versus `unknown`.

## Implementation boundary

Implementation is gated on agreement with #124, adoption and the UI owner, and
on a named owner for #126 in issue #4. When it starts it lives entirely under
`experiments/invention-lab/evaluator/**`: family files, the simulator wrapper,
the two calls and receipt encoding, with determinism, budget-refusal,
hidden-instance and `unknown` tests. It touches no provider, shared API,
manifest, kernel or UI code.

## Tests the implementation must pass

1. Same design, instance and evaluator version → identical trace digest.
2. Invalid designs (overlap, out of zone, over budget, unknown material, wrong
   language version) score `invalid` without simulating.
3. `experiment` refuses hidden instances and refuses once the budget token is
   spent; `score` refuses a method caller and refuses `comparison` before the
   salt is revealed.
4. A simulator crash or timeout yields `unknown`, with no automatic retry.
5. Revealed salts regenerate exactly the committed hidden instances.
6. A trivially bad design fails every family; a hand-built reference design
   succeeds on some development crossing instances (a sanity check, never used
   as a method or a baseline).
