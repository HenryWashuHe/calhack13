# Invention-lab lineage and evidence view (design)

**Status: proposed design plus an isolated, unwired read-only model.** Owner:
Eric (`EricZhu-31`) / Eric's Claude, as assigned in the
[handoff](../AGENT_HANDOFF.md#current-ownership-and-next-work). Scope is this
file and `apps/web/src/invention-lab/**`. Nothing here changes the kernel, forge,
executor, scoring, adoption, ACP bridge, `apps/stage` or the console's existing
routes. Wiring the view into `App.tsx` or adding a server route is shared work
and needs a separate claim in issue #1.

## Purpose

The [proposed demonstration](../INVENTION_LAB_DEMO.md) promises a view that
shows "the executable method responsible for each experiment, its ancestry, and
the receiving lab's acceptance decision", and displays **improved** only when
the declared comparison supports it. This view answers four questions for any
method version, from retained records only:

1. Where did it come from? (generation attempt, or revision of an exact parent)
2. What did producing it cost, including failed and unknown attempts?
3. Which organization checked it, and what did that organization conclude?
4. What kind of evidence supports each statement?

It is not a status dashboard for G0–G4 and it does not mark any gate complete.

## Inputs: existing retained records, unchanged

The view reads records that merged commands already write. It adds no new
record type, hash domain or authority.

| Record | Producer | Used for |
| --- | --- | --- |
| Generation status | `genesis-forge method-generate-status` ([generation](contracts/generation.md)) | Attempt node: org, operation, phase, settings, transport, actual model, usage, package ref, `authority` text |
| Generation checkpoint `completion.outcome` | Same checkpoint file | `kind` and enum `reason` only (e.g. `failed` / `protocol`). Returned text, prompts and request bodies are never read into the view |
| Revision observation `genesis-method-revision-observation/1` | `method-revise-observe` ([revision](contracts/revision.md)) | `revised-from` edge: exact parent → child refs, `source_changed`, org/operation |
| Adoption `Outcome` | `experiments/invention-lab/adoption` ([adoption](adoption.md)) | `adopted` / `not_applicable` / `insufficient_evidence` / `unknown` edge from a receiver org to an exact method ref |
| Scoring `Verdict` | `experiments/invention-lab/scoring` | Counts inside an adoption: candidate vs control successes of N, or the recorded reason |

| `ExecutionReceipt` | Lab executor (genesis-core `development_method`) | A run of an exact method: binding org/run/attempt, bound vs actually-run method, purpose, outcome, outputs, usage |
| `EvaluationReceipt` | Trusted scorer (genesis-core `development_evaluation`) | Scoring of one exact output: purpose, outcome, metrics ref or reason, usage |

Later inputs (roster/admission cost assessments, a predeclared comparison
report) extend the same model when their owners settle them. Until a comparison
record exists, the view has no path to "improved".

## Model

- **Node** = one exact method version, keyed by the full `ExactContentRef`
  (`domain`, `schema`, `id`, `version`, `sha256`). Equal id/version with a
  different digest is a different node, shown as such.
- **Attempt** = one generation operation. A failed, rejected, unknown or
  still-requesting attempt has no node but stays visible with its cost. Absence
  of a package is never zero cost.
- **Edges**, each carrying its source record:
  - `generated` — attempt → package ref (phase `generated` only).
  - `revised_from` — parent ref → child ref, with `source_changed`. A true flag
    means only "different source bytes".
  - `adoption` — receiver org → method ref, with the adoption outcome and,
    where present, the verdict counts. Only an `adopted` outcome names its own
    org and method (the installation). `not_applicable`,
    `insufficient_evidence` and `unknown` carry just a reason, so their
    receiver (`receiver.json` `org_id`) and method (`lab-adopt precheck`
    `offer`) are paired by the host. The view marks such edges "pairing
    supplied by host" and reports any installation that contradicts the
    supplied pairing as a problem. Adding receiver/offer refs to every outcome
    would be an adoption-owner decision, not part of this lane.

Organization identity is the recorded `org_id` string. The view does not claim
it is authenticated; [revision](contracts/revision.md) and adoption records say
the same.

## Display vocabulary

Labels come only from recorded values. Anything else renders as
**unrecognized** with the raw value, never as a guess.

| Recorded | Label | Never displayed as |
| --- | --- | --- |
| generation phase `generated` | generated source | executed, accepted, working |
| `failed` / `rejected_source` | generation failed / source rejected, plus the enum reason | "no cost", hidden |
| `requesting` / `needs_reconciliation` / outcome `unknown` | outcome unknown — do not replay | failed, empty |
| `prepared` | declared, not submitted | |
| `source_changed: true` | source changed (bytes) | improved |
| `source_changed: false` | source unchanged | |
| adoption `adopted` | adopted by *org* | improved, recommended to others |
| `not_applicable` | not applicable here (local checks rejected it) | broken |
| `insufficient_evidence` | insufficient evidence | rejected |
| `unknown` | unknown — retained, not concluded | rejected, empty |

Usage dimensions show `measured` values or **unknown** with the recorded
reason. A dimension absent from the record shows **not recorded**; it is not
zero. Output that equals the declared `max_tokens` is shown as "reached the
output cap" without asserting truncation as the failure cause.

## Evidence tiers

Each attempt and edge carries one tier, derived from fields, never from file
location or prose:

| Tier | Derived from |
| --- | --- |
| loopback fixture | transport `loopback_fixture` |
| unobserved | transport `unobserved` or no completion |
| operator-observed cloud | transport `anthropic_https`; hashes are local consistency, not provider authentication |
| local process | execution and evaluation receipts; host-recorded, not proof of isolation |
| independent comparison | a predeclared comparison report (future input; none exists) |

Tiers never merge upward: a local-process result next to a cloud generation is
two statements, not a "live run".

## Layout (first component)

One **attempt card** per generation operation: header with org and operation,
phase label and tier; a settings line (model, `sampling`, `max_tokens`); a usage
row with each dimension; package ref or "no package"; the record's own
`authority` sentence verbatim. A lineage panel that arranges cards and edges
A:v1 → B:v2 → A:local decision follows once execution receipts and a genuine
generated package exist. Until then, a lineage with one failed attempt and no
nodes is the honest picture.

## Execution and scoring receipts

A run is filed under the method the executor reports it **actually ran**. If
that differs from the method its frozen binding named, the run is still shown
there and the mismatch is reported. An evaluation attaches to the single run
whose outputs include its exact output ref and whose purpose (ordinary,
acceptance role, or comparison trial/arm) is identical. The view does not
recompute `execution_sha256`, so the attachment says "matched by output and
purpose". Zero or several matches leave the evaluation unattached and reported.

Execution outcomes read "output generated", "execution failed",
"execution interrupted" or "unknown — do not replay". "Output generated" is not
design success. Evaluation outcomes read "scored", "output invalid for
scoring", "scoring failed" or "scoring unknown"; "scored" means metrics exist,
while acceptance remains the adoption verdict. Until a model-generated method
is executed, these paths are exercised only by synthetic receipts shaped like
the Rust serializers.

## Attempt ledger

Status records may carry the optional enum-only `diagnostic` from
[generation](contracts/generation.md): the first rejected check and an
allowlisted stop reason. The card shows it as "rejected at *check*" with the
stop reason, and states that raw content was not retained, so the diagnostic
locates the rejection without explaining it. Values outside the contract's
lists render as unrecognized. Top-level status fields the decoder does not know
are listed as "unrecognized fields" instead of being dropped, so a later
contract addition is visible before this view interprets it.

The ledger lists every attempt and sums each usage dimension over measured
values. A total is exact only when every attempt measured that dimension;
otherwise it reads "≥ *sum* (*n* of *m* not measured)", and a dimension no
attempt measured reads "not recorded". Failed, rejected and unknown attempts
are included. For the two retained live attempts this reproduces the evidence
README's arithmetic total: 2 calls, 7999 input, 13084 output tokens and 98921 ms
request wall time, 0 packages. The ledger states that this is an arithmetic
total across separately declared attempts: not a predeclared combined budget,
a cost per success or a comparison.

## Non-goals and limits

- No writes, provider calls, replays or retries. No "run again" control.
- No new server route; the first wiring will load records the operator chooses.
- No ranking or recommendation of methods across organizations.
- Retained hashes are displayed for inspection; the view does not verify
  SHA256SUMS or chronology. Use the forge's read-only commands for that.
- Simulation outcomes are not real-world engineering evidence.

## Validation

`apps/web/src/invention-lab/*.test.ts(x)` decode the actual retained
[first live attempt](evidence/live-method-20261024/README.md) from the
repository bytes and check that it renders as an operator-observed cloud
generation failure with measured cost, a reached output cap and no package.
Additional cases cover unknown usage, unrecognized enum values, a revision
with unchanged source, and each adoption outcome. These are unit and static
render checks, not a browser walkthrough.
