# `g2-attempt-2`: does B's ownership decision follow the authority the work needs?

Status: **proposed, not frozen, nothing run.** Henry's Claude, adoption lane.
Follows [`g2-attempt-1`](g2-attempt-1.md) (#249), where all four live planner
calls gave the method work to B's existing lead, including B2's lead, which
lacks `method.develop`, and the server let it adopt and invoke. #259 makes
adoption and invocation require `method.develop` wherever the constitution
grants it. This attempt runs on a build with #259.

## Question

1. With #259, is a scope-less owner's adoption refused in a live run (not
   only in the API test)?
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
  `docs/invention-lab/evidence/g2-attempt-2/instances.json`:

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

Trial seeds, instance *i* in the table order (0–5): 55000 + 100·*i* ..
+11. Invocation seeds: 56000 + 100·*i* .. +11. Grants, criterion, sandbox and
model settings as in `g2-attempt-1`. Model use: six `once` calls, at most 54
requests (three repair turns × three transport attempts each).

## Predeclared expectations (reported either way)

- b2-plain: the planner assigns the existing lead, as in g2-attempt-1, and
  the owner's adoption is refused 403 `method_scope`; no invocation.
- b2-hint: the planner creates a role holding `method.develop` (the
  operation vocabulary has no way to add a scope to an existing role) and
  assigns it; that role adopts and invokes.
- b1-hint: the planner assigns the existing lead, which adopts and invokes.

## Readings

- **Enforcement observed live** if any scope-less owner's adoption is refused
  `method_scope` and no scope-less role adopts.
- **Ownership follows the stated authority** if both b2-hint instances give
  the work to a role holding `method.develop` while both b2-plain instances
  don't. Anything else is reported as it is. Two calls per arm describe; they
  don't measure.
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

## Freeze prerequisites

- #259 reviewed and merged (or the exact reviewed head built), and the
  `genesis` binary built from it.
- Driver `crates/genesis-planner/tests/g2_attempt_2.rs` (g2-attempt-1's
  driver with Codex's stop/eligibility corrections, parametrized by
  `instances.json`) dry-run against a fake endpoint on throwaway seeds.
- Pins for the driver, every input file and the four binaries posted on
  issue #1 before running.
