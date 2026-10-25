# `g2-attempt-2`: does B's ownership decision follow the authority the work needs?

Status: **proposed, not frozen, nothing run.** Henry's Claude, adoption lane.
Follows [`g2-attempt-1`](g2-attempt-1.md) (#249), where all four live planner
calls gave the method work to B's existing lead, including B2's lead, which
lacks `method.develop`, and the server let it adopt and invoke. #259 makes
adoption and invocation require `method.develop` wherever the constitution
grants it. This attempt runs on a build with #259. Revised after an
independent agent review (see the end).

## Question

1. With #259, is a scope-less owner's adoption refused in a live run (not
   only in the API test)? Given the server code this is close to certain
   once a scope-less role owns the work; what the live run adds is whether
   the planner hands it to such a role at all.
2. If B's own need text says the work requires `method.develop`, does the
   live planner give it to a role that holds it, by creating one, where the
   existing lead lacks it?

The second is about what the planner reads, not a planner capability claim.

## Design

Everything as in `g2-attempt-1` (A publishes its model-generated `parent`
`8215c367…`; each B instance is its own process, database, tokens and
receiver; one live `genesis-planner once` per instance; B's own receiving
trial vs its fixed cart, margin one, 12 rough-transport instances; owner asks
to adopt with the pinned trial; one invocation if adopted), except:

- Six B instances, each seeing `parent` only, declared in
  `docs/invention-lab/evidence/g2-attempt-2/instances.json` and run in that
  file's order, interleaved by arm (b2-plain-1, b2-hint-1, b1-hint-1,
  b2-plain-2, b2-hint-2, b1-hint-2) so a provider outage can't take out a
  whole arm:

  | instance | founding | need text |
  |---|---|---|
  | b2-plain-1, b2-plain-2 | `founding-b2.json` (lead: deliveries, `mission.coordinate` only) | as in g2-attempt-1 |
  | b2-hint-1, b2-hint-2 | `founding-b2.json` | as above + ". Adopting or running the method requires a role holding method.develop" |
  | b1-hint-1, b1-hint-2 | `founding-b1.json` (lead: evaluate reusable methods, holds `method.develop`) | with the same sentence |

- The sentence is B's own receiver-owned text in its intake request; no
  server or planner code changes for it.
- Two instances per arm, so a single unusual answer is visible as such.
- There is no `same-cart` offer.

## Fixed parameters

| instance | trial seeds | invocation seeds |
|---|---|---|
| b2-plain-1 | 55000..55011 | 56000..56011 |
| b2-plain-2 | 55100..55111 | 56100..56111 |
| b2-hint-1 | 55200..55211 | 56200..56211 |
| b2-hint-2 | 55300..55311 | 56300..56311 |
| b1-hint-1 | 55400..55411 | 56400..56411 |
| b1-hint-2 | 55500..55511 | 56500..56511 |

Grants, criterion and sandbox as in `g2-attempt-1`. Planner model:
`anthropic` / `claude-opus-5-5`, project settings, as in `g2-attempt-1`.
Model use: six `once` calls, at most 54 requests (three repair turns × three
transport attempts each).

The driver refuses to start, before creating anything or spending, unless the
four binaries match the pins posted on issue #1 (`EXPECT_*`) and the `genesis`
binary carries #259's scope check. It writes their digests and the provider
and model names (no key) to `run/provenance.json`. It runs the planner from
`/`, so no `.env` in the repository can reach it.

## Predeclared expectations (reported either way)

- b2-plain: the planner assigns the existing lead, as in g2-attempt-1, and
  the owner's adoption is refused 403 `method_scope`; no invocation.
- b2-hint: the planner creates a role holding `method.develop` (the
  operation vocabulary has no way to add a scope to an existing role) and
  assigns it; that role adopts and invokes.
- b1-hint: the planner assigns the existing lead, which adopts and invokes.
- "Adopts and invokes" also needs B's pinned trial to accept on its fresh
  seeds. A `not_applicable` there is about the method on that panel, not
  about scope, and is reported as such.

## Readings

- **Enforcement observed live** if any scope-less owner's adoption is refused
  `method_scope` and no scope-less role adopts.
- **Ownership follows the stated authority** if both b2-hint instances give
  the work to a role holding `method.develop` while both b2-plain instances
  don't. Anything else is reported as it is. Two calls per arm describe; they
  don't measure.
- **Delegated, not owned.** The planner may keep the lead accountable and
  delegate execution to a new role holding `method.develop`. That is a
  separate outcome: authority routed to the executor, not the owner. The
  owner's adoption is then refused by design, since the decision belongs to
  the owner. It counts neither for nor against the reading above. The
  summary records the responsibility's executor and its scopes for this.
- The enforcement observation in a b2-plain instance needs its pinned trial
  to be eligible. The driver stops before the decision on an ineligible trial
  (failed, interrupted or unknown execution or scoring), so such an instance
  gives no reading for question 1.
- In b2 the mismatched-grant control uses the scope-less lead. No control
  here shows a scope-holding role that isn't the owner being refused; none is
  claimed.
- One host, one operator, dev-stub providers, operator-reported receipts: not
  independent-organization evidence.
- Failed, interrupted or unknown executions are ineligible
  (`process-comparison/1`), as the driver already checks.

## Harness dry run (disclosed)

Driver at this commit, `genesis` built from #259 at `f52210a`, a fake model
endpoint (create a `method.develop` role when the need states the requirement
and the lead lacks it, otherwise assign the lead), throwaway seeds
63000../64000... All six instances completed: b2-plain owners refused 403
`method_scope`; b2-hint specialists and b1 leads adopted and invoked. No
planner judgment involved; not evidence; records not retained.

After the review: the revised driver, `genesis` `b6c825b0…` built from #259's
head `41153fc` (byte-identical to the `f52210a` build), the same fake
endpoint, throwaway seeds 65000../66000... in a scratch copy of this
directory. The same six outcomes, run in the interleaved order, with
`provenance.json` written and `decision_status`/`decision_error` recorded
(403 `method_scope` for both b2-plain). A wrong pin and a server binary
without #259's check each stop the driver before it creates the run
directory. Not evidence; records not retained.

## Independent review

An independent agent reviewed this protocol and the driver, read-only, before
freeze. It found nothing that crashes after spend or reuses seeds. It asked
for, and this revision adds: recording the executor and a predeclared
reading for delegation; pinned-binary, #259-marker and provenance checks
before any spend; running the planner outside the repository; interleaved
order; structured decision status; the attempt-1 regression guards (they
pass against this driver); and the wording above. It re-checked the seeds
(no overlap with any retained evidence), all 72 invocation instance
references, package digests and the 54-request bound.

## Freeze prerequisites

- #259 reviewed and merged (or the exact reviewed head built), and the
  `genesis` binary built from it.
- Driver `crates/genesis-planner/tests/g2_attempt_2.rs` (g2-attempt-1's
  driver with Codex's stop/eligibility corrections, parametrized by
  `instances.json`) dry-run against a fake endpoint on throwaway seeds.
- Pins for the driver, every input file and the four binaries posted on
  issue #1 before running.
