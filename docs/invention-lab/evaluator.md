# Invention lab: evaluator and task contract

**Status: reviewed contract proposal merged in #13, owned by Henry's core Claude
(assigned in this repository's issue #1; legacy task genesis#126). Runtime is not
merged or approved; branch-only `henry-claude/evaluator-impl` remains under review.**
It defines the simulated world, the task families, how instances are drawn and
hidden, how designs are scored, what a development experiment
costs and how all of that is identified. The development-method artifact and
its execution and scoring receipts are Codex's experimental contracts (#18,
#22; lane genesis#124); local adoption is
[the adoption proposal (#14)](https://github.com/HenryWashuHe/genesis-history-experiment/pull/14).
This document only references their types.

Nothing here is a result. Simulated success says nothing about a physical
machine.

## Roles and trust

| Party | May | May not |
| --- | --- | --- |
| Evaluator (this contract) | define families, draw instances, run the simulator, score, issue receipts | be changed, configured or called for scoring by a candidate |
| Development method (#18) | read development instances, request bounded simulation experiments, return candidate designs and a trace | read hidden instances or seeds, call `score`, change budgets, set its own success criterion |
| Receiving organization (adoption.md) | choose a suite of development instances and a frozen acceptance criterion, ask the evaluator to score | supply the evaluator to the method or reuse hidden instances for acceptance |
| Comparison runner (evaluator owner, trial plan from #28) | score every treatment privately on the committed hidden instances with the same evaluator version, then close the comparison and reveal | rerun against an exposed holdout until a favorable result appears |

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
- **Encoding:** the design digest is SHA-256 over the evaluator's own
  re-serialization of the parsed design (struct field order, `f32` in shortest
  round-trip form). This is local to the evaluator; it does not change the
  repository's `canonical_json` or any existing digest. The executor persists
  exactly these normalized bytes as the output artifact before the output is
  frozen, so the design digest and the retained output digest are the same
  bytes. The method's raw submission is kept separately in the transcript under
  its own digest. A design that fails
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
  the same trace digest on a **supported build and platform**, provided bodies,
  colliders and joints are inserted in the same order (design order, then the
  payload). The engine flag alone is not enough
  ([Rapier determinism notes](https://rapier.rs/docs/user_guides/templates_injected/determinism/)).
  The supported set is recorded with each evaluator version; so far only one
  x86_64 build pair (debug and release) has been checked. This covers simulation
  only; model calls inside a method are not replayable and their receipts
  belong to #18.
- Output per run: outcome, metrics, a downsampled trace (body poses at a fixed
  rate) for display, and the trace digest.

## Task families

A family is a parameterized generator plus a fixed success rule. An instance is
`(family_id, family_version, seed)`; the generator maps it to concrete terrain,
payload and disturbances. Families are versioned; changing a range or rule is a
new version.

Common to every family (version 1): build zone x ∈ [−6, 0] m, y ∈ [0, 3] m;
gravity 9.81 m/s²; fixed step 1/120 s. The **payload** is a 0.4 × 0.4 m box of
the instance's mass, friction 1.0, placed centered above the `payload_on`
body's top edge with a 0.01 m gap. Instance parameters are drawn from a
splitmix64 stream seeded by `seed` XOR a per-family constant and rounded to
millimetres; the exact generator is the family file of that evaluator version.
Progress is the payload's furthest x advance over the distance from its start
to the target line, clamped to [0, 1].

### `ravine-crossing` (A's visible task)

- **World:** near bank top at y = 0 for x ≤ 0; far bank top at y = step for
  x ≥ gap. gap ∈ [1.0, 2.5] m, step ∈ [−0.2, 0.2] m, payload ∈ [5, 30] kg,
  budget ∈ [4, 10], friction ∈ [0.5, 1.0]. Target line x = gap + 1.
- **Success:** the payload center stays past the target line and above
  y = step − 0.5 for 120 consecutive steps.
- **Termination:** success, or payload center below y = −1.5 (fell into the
  gap: failure), or 2400 steps.

### `rough-transport` (B's related task)

- **World:** flat ground at y = 0 with 0.2 m-wide fixed bumps from x = 0.5 m,
  height ∈ [0.10, 0.35] m, spacing ∈ [0.6, 1.5] m; target x = 8 m.
  1–4 impulses on the payload at seeded steps in [120, 1800],
  dx ∈ [−8, 8], dy ∈ [0, 5] N·s. Payload ∈ [5, 30] kg, budget ∈ [4, 10],
  friction ∈ [0.5, 1.0].
- **Success:** payload center reaches x = 8 m before its bottom touches the
  ground (within 0.02 m).
- **Termination:** success, ground touch (failure), or 2400 steps.

### `return-mix` (A's fresh return challenge)

- **World:** `ravine-crossing` geometry with friction ∈ [0.3, 0.8] and 1–3
  impulses as in `rough-transport`. Success and termination as for ravine.
- Scored only on hidden instances in a declared comparison.

### `projectile-target` (a different-task control)

- **World:** flat ground at y = 0 and a fixed 1 m-high wall at x = 1.5 m that
  the launcher can't pass; target zone center ∈ [4, 9] m, half-width 0.5 m.
  Payload ∈ [1, 5] kg, budget ∈ [4, 10], friction ∈ [0.5, 1.0].
- **Success:** at the end of 1200 steps the payload center is inside the zone
  and below y = 1.
- **Not a proof of inapplicability.** A different family label doesn't make a
  method inapplicable: a generic experiment-search method could legitimately
  transfer. Here, transfer is an **empirical result**, scored like any other
  (an organization that declines and then fails the task has not succeeded).
  Inapplicability is established only by an independent precondition, for
  example a method whose declared environment needs design language 2 or a
  sensor this language doesn't have.

These are proposals for the first families. Exact ranges are fixed in the
family files before any comparison, and no family is tuned to make a planned
method revision win.

## Development versus hidden instances

- **Development instances** are published seeds below 2³², with full
  parameters. Methods and acceptance suites use only these.
- **Hidden instances** come from a secret salt held only by the evaluator
  owner. Their seeds always have the top bit set, so they can't collide with a
  development seed.
  - Fields are encoded unambiguously: each string as its UTF-8 length (u64
    big-endian) followed by its bytes; integers as u64 big-endian. Write
    `enc(x, …)` for the concatenation. So `("a:b", "c")` and `("a", "b:c")`
    can't collide.
  - Commitment descriptor (immutable once published): protocol
    `genesis-lab-hidden/2`, comparison id, family, family version, draw count
    and `commitment = sha256(enc("genesis-lab-hidden/2", comparison_id, salt,
    family, family_version, count))`. The plan's hidden-set reference is this
    descriptor and never changes.
  - Reveal is separate evidence: the salt, checked by recomputing the
    commitment against the unchanged descriptor.
  - Draw: seed *i* for *i* = 0…count−1 is the first 8 bytes (little-endian) of
    `sha256(enc("genesis-lab-seed/2", comparison_id, salt, family,
    family_version, i))`, with the top bit set. Order is by *i*.

### Holdout timeline

1. Publish the comparison identity, count and commitment. Freeze each method.
2. Reveal the public development inputs (fresh-task development instances).
3. Run bounded development; every attempt, including no-output ones, goes in
   the plan's ledger.
4. Stop every candidate process; freeze the produced designs and transcripts.
5. The evaluator owner scores them **privately** on the hidden instances.
6. Close the comparison: no more scoring requests are accepted for it.
7. Reveal the salt. Anyone can regenerate the hidden seeds and re-run scoring.

Hidden instances never reach a method or organization before step 7, and are
used for exactly one comparison. Development inputs (step 2) are separate from
the hidden scoring perturbations.

## Interfaces

Two calls, both on the evaluator process. They are expressed in the types of
the experimental contracts in #18 (`development_method`, execution) and #22
(`development_evaluation`, scoring); see
[identity-and-execution.md](contracts/identity-and-execution.md) and
[evaluation.md](contracts/evaluation.md), both on main. The merged
[trial plan](contracts/trials.md) (#28) and
[experiment ledger](contracts/experiments.md) (#32) supply the bounded run scope.

- `experiment(design, instance_ref, grant) -> ExperimentResult`
  - Development only, inside one method execution. The current #28/#32
    contracts support `acceptance` and `comparison` slots; `instance_ref` must
    be one of the frozen public development instances in that slot's plan.
    Comparison arms also develop on public instances. #18 has an `ordinary`
    purpose, but ordinary-run scope/accounting is a future extension, not
    supported by TrialPlan v1. Hidden instances are never reachable through
    this call; hidden scoring is the separate trusted `score` operation.
  - Returns the simulation outcome, metrics, trace digest and downsampled trace.
    These feed the method's own transcript; they are not score evidence.
  - Debits the episode's **simulation steps** from the execution's grant
    (`simulation_steps` in #18's resource grant; fixed timestep, so steps are
    exact and integer). Refuses when the grant is spent. The method can't
    enlarge the grant. Usage the evaluator can't measure is reported
    `unknown`, never zero.
- `score(request) -> scoring receipt`
  - Takes a frozen #22 `EvaluationRequest`: the full execution receipt (and
    with it run, attempt, method, task and grant), the exact retained output
    (the design), the scoring context and a separate scoring grant. Callers:
    an organization's acceptance check or the evaluator owner's comparison
    runner. Never a method.
  - Purpose comes from the execution: `acceptance` (development instances
    only) or `comparison` (hidden instances; scored privately by the salt
    holder during step 5, before the reveal).
  - The receipt binds what #22 binds plus the acceptance or comparison plan,
    trial and arm. Which requests must exist comes from the plan's declared
    roster in #28; #32 binds cumulative public calls and their transcript.
    Trusted runtime integration must ensure a replay or favorable re-run cannot
    silently change a comparison. A content digest identifies the evaluator;
    it does not authenticate it.

### How this evaluator fills #22's scoring context

| #22 field | This evaluator |
| --- | --- |
| scoring environment | evaluator id@version with digest over engine version, scoring code and material/motor tables |
| task generator | family id@version and digest of the family file |
| instance-set reference | for development: the published seed list; for hidden sets: the immutable commitment descriptor above (the reveal is stored separately and never edits this reference) |
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
- family id/version and seed; for hidden instances the commitment
  descriptor (the reveal is a separate record that points to it)
- simulation steps and wall time consumed, measured or `unknown`

Model settings and method identity stay in #18's execution receipt, which the
scoring request binds in full. #18 has no installation id; an organization's
installation of a method is adoption evidence (see adoption.md), not part of
#18.

## Budget and cost accounting

All units are integers. Per development task, the receiving organization's
policy sets the #18 grant: simulation steps (metered and enforced here) and
wall time, model calls and tokens (enforced by the method executor). Scoring
has its own grant (#22).

Every `experiment` and `score` call carries a request identity (execution run
and attempt plus a per-execution sequence number, or the #22 request id).

| Case | Charge |
| --- | --- |
| Before running | reserve the family's full episode steps against the grant; refuse if they don't fit (charged 0) |
| Hidden instance on `experiment`, closed comparison, wrong purpose | refused, charged 0 |
| Invalid design | 0 steps measured; the attempt still appears in the transcript |
| Ran to termination | measured steps charged; the rest of the reservation released |
| Simulation diverged (`Failed`) | measured steps up to divergence |
| Crash or timeout (`Unknown`) | usage `unknown`; the full reservation stays consumed, never refunded as zero |
| Replay of an already answered request id | the stored result, charged nothing more; a different payload under the same id is refused |

Replay storage is a runtime responsibility; this evaluator is stateless per
call. A separate experiment-count ceiling isn't needed: steps bound the work.

The comparison counts everything: cold method development, failed attempts and
experiments, not only the final design's run. Equal total budgets across
treatments are the primary comparison; per-task warm numbers are secondary.

## What this needs from other contracts

- **#18/#22 (Codex):** method I/O types carrying `instance_ref`s and designs;
  the executor passing the grant's `simulation_steps` to the evaluator; the
  merged #28 pre-execution trial plan listing the instance identities each
  trial uses, so no-output failures stay in the denominator; and #32
  reserve/settle/seal accounting and exact execution-transcript validation.
  These libraries do not provide the trusted runtime integration themselves.
- **adoption.md:** acceptance suites are lists of development `instance_ref`s
  plus a criterion over scoring metrics (for example `success` count versus the
  control arm); the evaluator requirement is `evaluator_id@version` **plus its content
  digest**, resolved and pinned by the receiver; a digest change needs
  re-acceptance even under the same version label.
- **Evidence UI (#123):** the downsampled trace for animation, score receipts
  for the outcome panel, and the distinction `failed` versus `unknown`.

## Implementation boundary

Implementation is gated on interface agreement with #18/#22, adoption (#14)
and the UI owner. It lives entirely under
`experiments/invention-lab/evaluator/**`: family files, the simulator wrapper,
the two calls and receipt encoding, with determinism, budget-refusal,
hidden-instance and `unknown` tests. It touches no provider, shared API,
manifest, kernel or UI code.

## Tests the implementation must pass

1. Same design, instance and evaluator version → identical trace digest.
2. Invalid designs (overlap, out of zone, over budget, unknown material, wrong
   language version) score `invalid` without simulating.
3. `experiment` refuses hidden instances for every purpose, accepts
   comparison-purpose development on public instances, and refuses once the
   cumulative grant is spent (the grant is owned by the trusted ledger, never
   by a caller-supplied balance). `score` refuses a method caller; comparison
   scoring is the salt holder's private step and refuses a salt that doesn't
   open the unchanged commitment descriptor, and any request after the
   comparison closes. Disclosure happens only after every candidate process in
   the comparison has stopped and its outputs and transcripts are frozen.
4. A simulator crash or timeout yields `Unknown`, with no automatic retry; a
   design that fails its task yields `Scored` with `success: false`.
5. Revealed salts regenerate exactly the committed hidden instances.
6. A trivially bad design fails every family; a hand-built reference design
   succeeds on some development crossing instances (a sanity check, never used
   as a method or a baseline).
