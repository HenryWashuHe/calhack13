# Experimental independent scoring records

Status: proposed, typed contract implemented in
[`development_evaluation.rs`](../../../crates/genesis-core/src/development_evaluation.rs)
on top of the [identity and execution contract](identity-and-execution.md).
No evaluator, task simulator, persistence, transport, holdout isolation, budget
enforcement or runtime integration is implemented by this module. G0 remains
open until the affected owners agree on the interfaces and runtime; this is not
G4 comparison evidence. Current ownership lives in [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).

## One execution, selected output, separate scoring evidence

`EvaluationRequest` freezes a scoring request after a particular output exists
and before scoring is requested. It binds the **entire execution receipt** by a
domain/version-separated canonical SHA-256 digest. Outputs, transcript, outcome
and measured or unknown consumption are included, in addition to the execution
binding. Changing execution evidence without changing the run ID still changes
this digest. The selected output must match a retained execution output exactly,
including its content digest, schema and version.

The request copies the execution's task, trusted evaluator identity and purpose.
Ordinary execution cannot be reclassified as acceptance or comparison by the
scorer. Its scoring context pins the scoring environment, task generator,
instance-set reference, design schema, metric schema and scoring rule separately.
The scoring grant is separate from the development grant. These are opaque exact
references: the trusted runtime must fetch and validate the referenced contents,
including schema compatibility, and establish their authority.

Callers retain the frozen request, execution and original run binding separately
from any incoming receipt. `validate_against` checks against that trusted state.
Recomputing an altered request's hash does not authorize replacing it. Hashes
provide content identity, not signatures, authorship, execution proof or authority.
Instance-set hashes do not implement secrecy, commitment/reveal or independent
holdout selection; task authors must not disclose hidden content to developers.

## Outcomes and accounting

| Scoring outcome | Required record | Interpretation |
| --- | --- | --- |
| `Scored` | Exact observed context, metrics reference and trace | A metrics artifact is reported; no quality threshold or success is inferred |
| `Invalid` | Exact observed context, nonblank reason and trace | Design invalidity reported by the bound evaluation context |
| `Failed` | Nonblank reason and trace/local attempt log | Known scoring failure; observed context may be absent if the worker never started |
| `Unknown` | Nonblank reason and trace/local reconciliation log | Outcome is unresolved; no permission to resubmit is granted |

If any observed context is present, every field must match the trusted request,
even for failed or unknown outcomes. The independent development outcome remains
visible: scoring a retained partial output from a failed run does not make the
development run successful. No-output development failures cannot form a scoring
request; they must remain in the pre-execution trial ledger described below.

`EvaluationAccounting` returns separate development and scoring assessments.
`WithinLimits` means every reported dimension was measured and within the grant;
it is not proof that measurements are authentic or that runtime limits were
enforced. `Unknown` remains explicit, even when metrics exist. A measured overrun
rejects validation despite other dimensions being unknown. Invalid receipts are
errors for the caller to preserve in its audit log, never a reason to silently
remove a trial from a denominator.

## Scoring-request roster, not a comparison trial plan

`assess_request_roster` returns one assessment per supplied frozen request, in
declared order, with missing, scored, invalid, failed and unknown statuses.
Missing receipts have no scoring accounting, rather than zero consumption.
Unexpected or duplicate receipt IDs, duplicate request IDs and duplicate
comparison `(exact plan, trial_id, arm_id)` slots reject the roster. If a trial
requires multiple scored outputs, that selection and aggregation protocol needs
a separate versioned contract; changing request IDs cannot create extra trials.
Replay deduplication and durable ingestion are runtime responsibilities.

This roster starts **after outputs have been selected**. It cannot show that all
development trials were attempted or that output selection was fair. A caller
could omit unrequested or no-output failures. The API deliberately provides no
aggregate comparison-complete, pass, benefit or adoption-authority flag.

Before closing G4, a separate plan must freeze trial/arm/task identities **before
development**, including model/settings, task order and isolation, attempt/output
selection policy, all resource grants and holdout lifecycle. Its ledger must
retain no-output, interrupted, failed and unknown development attempts, as well
as scoring failures and unknown usage. It must specify how those results enter
the declared analysis rather than dropping them. Fixed model weights, fair total
budgets, fresh tasks, independent scoring, method derivation and a successful
A-to-B-to-A return chain all remain unproved by this contract.

## Validation

Run `cargo test -p genesis-core --test development_evaluation`. Regressions cover
full execution evidence substitution, exact output membership, purpose changes,
self-consistent request substitution, observed context mismatches, unknown usage
and overruns, all roster statuses, duplicate/extra/relabeled trials, no-output
failures, and strict wire decoding. These are local contract tests only.
