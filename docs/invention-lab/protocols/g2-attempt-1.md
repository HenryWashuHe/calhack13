# `g2-attempt-1`: B inherits A's exact method, with a live planner and controls

Status: **proposed, not frozen, nothing run.** Henry's Claude, evaluator and
adoption lane. Written after rehearsals 5 and 6 (#230, #248), which exercised
this path with a stand-in and, in rehearsal 6, a live planner. This is
exploratory evidence for the G2 row of [the goal](../../ONGOING_GOAL.md), not
the controlled G4 comparison. A decline or a planner refusal is a result.
Revised after an independent agent review (#249) found that the first draft
let one offer's planner call shape the other's, and could crash after model
spend.

## Question

Does B, as a separate organization, discover A's exact generated method,
check it locally on B's own distinct task, give the work a justified owner
through its live planner, adopt it only if B's own check passes, and use it
under B's grant with a scored outcome? And does B's receiver gate refuse an
offer that doesn't beat B's baseline?

## Organizations and methods

- **A (`org_example_a`)** publishes two methods to the registry with A's
  token:
  - **`parent`**: A's model-generated method `8215c367…` (#95), developed for
    ravine-crossing.
  - **`same-cart`** (`b1a3bc73…`): a handwritten method that submits exactly
    B's fixed cart `dc5b74f3…`'s design under its own method ID. It ties the
    fixed cart on every instance by construction, so margin one rejects it.
    It is a **plumbing check of B's receiver gate**, not a test of B's
    judgment of value.
- **B (`org_example_b`)**: **four independent instances**, each its own
  process, database, tokens (`<instance>-b-operator` etc.) and receiver, all
  with the same organization ID. Each instance sees **exactly one offer**, so
  no planner call sees another's outcome:
  - **B1-parent, B1-same-cart**: founded with `founding-b1.json`. The Lab
    lead's charter is "Own the lab and evaluate reusable methods" and it
    holds `method.develop`.
  - **B2-parent, B2-same-cart**: founded with `founding-b2.json`. Identical
    except the lead's charter is "Own the lab and run rough-transport
    deliveries" and it holds only `mission.coordinate`. The constitution still
    allows `method.develop`, so the planner can grant it to a new role.
  - B1 and B2 differ in **both** charter and scopes, and no server code
    requires `method.develop` for adoption, so the contrast is about what the
    planner reads in B's organization; it can't be pinned on either change
    alone.

## Stages (per B instance)

1. (Once, shared.) A publishes `parent` and `same-cart`. Controls: B's reader
   token publishing as A is refused; a bearer token A never issued is refused
   on A.
2. B searches the registry and posts one intake for its offer, need "Develop
   rough-transport cart designs with a method another lab published",
   acceptance "Beats the fixed cart by one success on twelve fresh public
   instances".
3. **Live planner.** One `genesis-planner once` call for that observation,
   project settings (`anthropic`, `claude-opus-5-5`). The requirement is
   matched by its triggering evidence and counts as committed only once
   `ASSIGNED` with a responsibility. Anything else (no change, escalation, a
   requirement left unmet after kernel rejections) is recorded and the
   instance stops there.
4. **B receiving trial**: the offer vs B's pinned fixed cart, rough-transport,
   12 instances, margin one. Reported only: the offer vs random search
   `06777e8e…` on the same panel. A pinned trial without a settled verdict
   (failed, interrupted, unknown) is ineligible and gets no decision. B
   restarts with a receiver pin for exactly that plan.
5. **Decision.** Controls first: a bearer token B never issued; and a
   mismatched grant (when the lead owns the work, the lead with its unrelated
   "Coordinate the lab" responsibility; otherwise the lead for the owner's
   responsibility). Both must be refused. Then the owner **asks to adopt with
   the pinned trial**, and B's receiver decides from that evidence: `adopted`
   or `not_applicable`.
6. **Use.** Only if adopted: one invocation under B's grant on 12 fresh
   rough-transport instances (plan instances, instance set and criterion from
   the template), scored and attached to B's requirement, responsibility,
   role and adoption.

A stop at any stage in one instance is recorded in its summary and never
prevents the other instances.

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

Model use: four `once` calls. Each may take up to three repair turns, and
each turn up to three transport attempts, so **at most 36 requests**; a
normal run is four. No other model use. The planner records no token usage;
the README records prompt sizes.

## Predeclared expectations (reported either way)

Written after the first dry run below, so I knew `parent` does well on
rough-transport and `same-cart` ties.

- `parent`: adopted in both B1-parent and B2-parent; the invocation completes
  and is scored.
- `same-cart`: rejected by the pinned trial, recorded `not_applicable`, no
  invocation.
- Ownership: B1 instances assign to the existing lead; B2 instances create a
  specialist or assign to a role holding `method.develop`. If B2 assigns to
  the lead anyway, that's the outcome, not corrected.

## Readings

- **G2-relevant:** `parent` adopted and used with a scored result in at least
  one instance, `same-cart` refused by the receiver gate, and the ownership
  decisions recorded with their stated reasons. Even then: one host,
  loopback, dev-stub providers, operator-run, so this is not
  independent-organization evidence, and receipts stay operator-reported.
- **Ownership contrast:** described, not scored. Four calls are not a
  measurement of planner judgment.
- Failed, interrupted or unknown executions are ineligible, as in
  `process-comparison/1`.

## Harness dry runs (disclosed)

1. Before the review: the first driver, a **fake model endpoint** answering
   every call with an existing-owner assignment, seeds 53000..53711. I saw
   the outcomes: `parent` beat the fixed cart 12/12 vs 6/12 (B1) and 11/12 vs
   5/12 (B2); `same-cart` tied 5/12 vs 5/12. Those seeds are consumed; this
   attempt uses 54000..54711.
2. After the review: the revised driver, a fake endpoint cycling through
   assign, assign, no change and unparseable output, throwaway seeds
   61000..61711. All four paths were recorded without a crash (adopted with an
   attached invocation; `not_applicable`; no change; escalation after three
   unusable turns), and both controls were refused (401, 409). The reviewer's
   re-check then found panics still reachable after spend (server start-up
   wait, HTTP send); those now return recorded stops, and the same cycle on
   throwaway seeds 62000..62711 reproduced the four outcomes.

Neither involved planner judgment; neither is evidence; their records are not
retained.

## Freeze

At freeze the predeclaration comment on issue #1 pins: the branch commit, the
driver's sha256, every file in `docs/invention-lab/evidence/g2-attempt-1/`
(specs, invocation templates, founding requests, `same-cart` package), and
the `genesis`, `genesis-planner`, `lab-exec` and `lab-score` binary hashes.
Review: an independent agent's review (#249) in place of Codex and Eric, who
were paused; they can contest the run afterwards.
