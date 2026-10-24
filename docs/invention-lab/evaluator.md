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

Two calls, both on the evaluator process. They are expressed in the types of
the experimental contracts in #18 (`development_method`, execution) and #22
(`development_evaluation`, scoring); see
[identity-and-execution.md](contracts/identity-and-execution.md) and
[evaluation.md](contracts/evaluation.md) on those branches.

- `experiment(design, instance_ref, grant) -> ExperimentResult`
  - Development only, inside one method execution (#18 purpose `ordinary` or
    `acceptance`). `instance_ref` must be a development instance.
  - Returns the simulation outcome, metrics, trace digest and downsampled trace.
    These feed the method's own transcript; they are not score evidence.
  - Debits the episode's **simulation steps** from the execution's grant
    (`simulation_steps` in #18's resource grant; fixed timestep, so steps are
    exact and integer). Refuses when the grant is spent. The method can't
    enlarge the grant. Usage the evaluator can't measure is reported
    `unknown`, never zero.
- `score(request) -> scoring receipt`
  - Takes a frozen #22 `EvaluationRequest`: the full execution receipt, the
    exact retained output (the design), the scoring context and a separate
    scoring grant. Callers: an organization's acceptance check or the
    comparison runner. Never a method.
  - Purpose comes from the execution: `acceptance` (development instances
    only) or `comparison` (hidden instances, only after the salt for that
    comparison is revealed).

### How this evaluator fills #22's scoring context

| #22 field | This evaluator |
| --- | --- |
| scoring environment | evaluator id@version with digest over engine version, scoring code and material/motor tables |
| task generator | family id@version and digest of the family file |
| instance-set reference | for development: the published seed list; for hidden sets: the salt commitment, with the salt added once revealed |
| design schema | design language v1 |
| metric schema | per family: `success` (bool), `progress` (0–1), `cost`, `joints_broken`, `time_to_target` |
| scoring rule | the family's success rule, versioned with the family |

### Outcomes

Design quality is a **metric**, not a scoring outcome:

| #22 outcome | Here |
| --- | --- |
| `Scored` | the simulation ran; metrics include `success: true` or `false` |
| `Invalid` | the design failed validation (overlap, out of zone, over budget, unknown material, wrong language version); not simulated |
| `Failed` | the scoring worker failed in a known way (for example the engine refused the world) |
| `Unknown` | crash, timeout or lost result; not silently retried |

So a design that drops the payload is `Scored` with `success: false`, not
`Failed`. Earlier drafts of this document used `succeeded`/`failed` as outcomes;
that conflated design failure with scoring failure.

## Receipts

The #22 scoring receipt carries the observed scoring context, outcome, metrics
reference and trace. In addition this evaluator records:

- design-language version and design digest (also the selected output's digest)
- family id/version and seed; for hidden instances the salt commitment and,
  after reveal, the salt
- simulation steps and wall time consumed, measured or `unknown`

Model identity, method identity and installation id stay in #18's execution
receipt, which the scoring request binds in full.

## Budget and cost accounting

Per development task, the receiving organization's policy sets the #18 grant:

- simulation steps (metered and enforced by this evaluator),
- wall time, model calls and tokens (enforced by the method executor, #18).

A separate experiment-count ceiling is not needed: steps bound the work, and
the transcript records each experiment.

The comparison counts everything: cold method development, failed attempts and
experiments, not only the final design's run. Equal total budgets across
treatments are the primary comparison; per-task warm numbers are secondary.

## What this needs from other contracts

- **#18/#22 (Codex):** method I/O types carrying `instance_ref`s and designs;
  the executor passing the grant's `simulation_steps` to the evaluator; the
  pre-execution trial plan (still pending in #22) listing the instance
  identities each trial uses, so no-output failures stay in the denominator.
- **adoption.md:** acceptance suites are lists of development `instance_ref`s
  plus a criterion over scoring metrics (for example `success` count versus the
  control arm); the evaluator requirement a method declares is
  `evaluator_id@version`.
- **Evidence UI (#123):** the downsampled trace for animation, score receipts
  for the outcome panel, and the distinction `failed` versus `unknown`.

## Implementation boundary

Implementation is gated on agreement with #124, adoption and the UI owner, and
on the #126 owner (assigned to Henry's core Claude in this repository's issue #1). When it starts it lives entirely under
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
4. A simulator crash or timeout yields `Unknown`, with no automatic retry; a
   design that fails its task yields `Scored` with `success: false`.
5. Revealed salts regenerate exactly the committed hidden instances.
6. A trivially bad design fails every family; a hand-built reference design
   succeeds on some development crossing instances (a sanity check, never used
   as a method or a baseline).
