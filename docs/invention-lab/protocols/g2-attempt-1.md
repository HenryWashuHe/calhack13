# `g2-attempt-1`: B inherits A's exact method, with a live planner and controls

Status: **proposed, not frozen, nothing run.** Henry's Claude, evaluator and
adoption lane. Written after rehearsals 5 and 6 (#230, #248), which exercised
this path with a stand-in and, in rehearsal 6, a live planner. This is
exploratory evidence for the G2 row of [the goal](../../ONGOING_GOAL.md), not
the controlled G4 comparison. A decline or a planner refusal is a result.

## Question

Does B, as a separate organization, discover A's exact generated method,
check it locally on B's own distinct task, give the work a justified owner
through its live planner, adopt it only if B's own check passes, and use it
under B's grant with a scored outcome? And, in the same run, does B decline
an offer that doesn't beat what B already has?

## Organizations and methods

- **A (`org_example_a`)** publishes two methods to the registry with A's
  token:
  - **`parent`**: A's model-generated method `8215c367…` (#95), developed for
    ravine-crossing. On rough-transport it has scored 6/6 in #116 (against the
    child), and the fixed cart 2/6, so the expected outcome below is adopt.
  - **`same-cart`**: a handwritten method that submits exactly B's fixed cart
    `dc5b74f3…`'s design under its own method ID. It cannot beat B's pinned
    control by one success, so the expected outcome is decline. It is the
    non-reuse control: an offer no better than B's baseline.
- **B (`org_example_b`)**, two independent instances, each its own process,
  database, tokens and receiver:
  - **B1**: constitution as in rehearsals 3–6. The Lab lead's charter
    ("evaluate reusable methods") covers method evaluation and it holds
    `method.develop`.
  - **B2**: identical except the lead is founded with charter "Own the lab
    and run rough-transport deliveries" and authority scopes
    `["mission.coordinate"]` only, so it does **not** hold `method.develop`.
    B2's constitution still allows `method.develop`, so the planner can grant
    it to a new role. Nothing else differs.
  - The pair is the existing-owner contrast: whether the live planner's
    ownership decision follows B's own organization rather than a default.

## Stages (per B instance)

1. A publishes `parent` and `same-cart`. Controls: B's reader token publishing
   as A is refused; B's operator token on A is refused.
2. B searches the registry and posts **two** intakes, one per selected offer,
   with the need "develop rough-transport cart designs with a method another
   lab published" and B's acceptance "beats the fixed cart by one success on
   twelve fresh public instances".
3. **Live planner.** One `genesis-planner once` call per intake observation,
   project settings (`anthropic`, `claude-opus-5-5`), whatever it decides is
   recorded with its rationale. No retry beyond the client's two transient
   retries. If it makes no change or escalates for an offer, that offer stops
   there.
4. **B receiving trial** for each committed requirement: offer vs B's pinned
   fixed cart, rough-transport, 12 instances, margin one. Reported only: offer
   vs random search `06777e8e…` on the same panel. B restarts with a receiver
   pin for exactly that plan.
5. **Owner decision.** The requirement's owner adopts if the pinned trial is
   Accepted and declines otherwise. Controls: A's token on B refused; the lead
   refused when it doesn't own the work.
6. **Use.** Only if adopted: one invocation under B's grant on 12 fresh
   rough-transport instances, scored with `lab-eval accept`, result attached
   to B's requirement, responsibility, role and adoption.

## Fixed parameters

| | B1 | B2 |
| --- | --- | --- |
| trial seeds, `parent` | 54000..54011 | 54100..54111 |
| trial seeds, `same-cart` | 54200..54211 | 54300..54311 |
| invocation seeds, `parent` | 54400..54411 | 54500..54511 |
| invocation seeds, `same-cart` (only if adopted) | 54600..54611 | 54700..54711 |
| development grant | 72,000 steps, 60,000 ms, zero model allowances | same |
| scoring grant | 600,000 ms, 12 × 2,400 steps | same |
| criterion | margin one | same |
| sandbox | namespaces, pinned Python 3.14.0 | same |

Model use: at most four planner calls (two offers × two B instances), each
with the client's two transient retries. No other model use. The planner
records no token usage; the README records prompt sizes.

## Predeclared expectations (reported either way)

- `parent`: B adopts in both instances; the invocation completes and is
  scored.
- `same-cart`: B's trial rejects (tie or worse), B declines, no invocation.
- Ownership: B1 assigns to the existing lead; B2 creates a specialist or
  assigns to a role that holds `method.develop`. If B2 assigns to the lead
  anyway, that's recorded as the outcome, not corrected.

## Readings

- **G2-relevant:** `parent` adopted and used with a scored result in at least
  one B instance, `same-cart` declined, and both ownership decisions recorded
  with their stated reasons. Even then: one host, loopback, dev-stub
  providers, operator-run, so this is not independent-organization evidence,
  and receipts stay operator-reported.
- **Ownership contrast:** described, not scored. Two calls are not a
  measurement of planner judgment.
- Failed, interrupted or unknown executions are ineligible, as in
  `process-comparison/1`.

## Harness dry run (disclosed)

Before freezing I ran the driver once end to end with a **fake model
endpoint** (a loopback server answering every planner call with an
existing-owner assignment) to check the harness, on seeds 53000..53711. That
consumed those seeds and I saw their outcomes: on rough-transport the
`parent` beat the fixed cart 12/12 vs 6/12 (B1) and 11/12 vs 5/12 (B2), and
`same-cart` tied the fixed cart 5/12 vs 5/12 and was declined. No planner
judgment was involved. The attempt therefore uses fresh seeds 54000..54711,
and these numbers are not evidence for it. The dry run's records are not
retained.

## Prerequisites before freeze

- Review by Codex (registry/planner contracts) and any objection from Eric.
- Driver `crates/genesis-planner/tests/g2_attempt_1.rs` (rehearsal 6's,
  generalized to two B instances and two offers, with the lead-control fix
  and #245's restart ordering); `same-cart` package
  `b1a3bc73…`; specs and invocation templates in
  `docs/invention-lab/evidence/g2-attempt-1/`.
- The B1/B2 founding requests (above) copied into this directory verbatim.
