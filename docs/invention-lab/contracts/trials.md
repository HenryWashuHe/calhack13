# Experimental pre-execution trial batches

`development_trial` extends the [execution](identity-and-execution.md) and [scoring](evaluation.md) records. It validates a frozen single-task batch; it is not a runner, enforcement service, acceptance verdict or comparative result.

## What freezes

A plan pins the ordered slots, methods/dependencies, organization/run/attempt/request IDs, task, evaluator, model settings, environments, public development instances, scoring context, criterion and separate development/scoring grants. Its domain-separated content reference becomes every run's purpose reference. Changing any field invalidates earlier evidence against the new plan.

Acceptance has exactly one candidate and one control in the same receiving organization. Comparison repetitions have the same set of at least two arms; each arm keeps its organization, method and dependencies. All slots share the declared task, settings and grants. The criterion is an immutable artifact reference, not a verdict executed by this module.

Schema v1 allows one attempt and at most one retained output per slot. Internal metered design search is allowed. Post-score selection among retained attempts/outputs, retries and evolving treatments require another protocol. Missing evidence does not authorize a retry.

## Hidden instances and authority

Comparison `context.instance_set` identifies an immutable hidden-set commitment descriptor; `HiddenCommitment.protocol` pins its protocol. The descriptor must bind generator/domain, comparison identity and ordered draw count. Reveal data is separate evidence and must never mutate that reference. Acceptance uses public development instances; trusted loaders must verify referenced artifacts and their compatibility with the explicit public list.

The runtime must persist the plan before development, exclude hidden content from candidates, stop all affected candidate execution and freeze outputs/transcripts before disclosure. Prefer private scoring and public reveal after comparison closure. This module verifies neither reveal preimages nor that ordering. Hashes alone establish no authorship, authority, confidentiality or execution.

## Evidence retention

`assess_trials` returns every declared slot in plan order, including missing execution and failed/interrupted/unknown runs without output. Development outcome, output presence and development/scoring accounting stay separate. It reconstructs scoring expectations from the plan and validated execution, rather than trusting an incoming scorer's request. Extra/duplicate slots, changed attempts, multiple outputs and substituted scoring records reject validation; callers retain those errors in their audit ledger.

There is no aggregate pass or improvement flag. Equal grants per slot do not establish equal total cost: cold method creation, prior search, scheduling, hidden-set freshness, experiment-call transcripts and other overhead still require trusted accounting. The single-task batch does not implement the A-to-B-to-A chain, autonomous method development or a full G4 comparison. Runtime and evaluator ownership remain in [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).

Validation: `cargo test -p genesis-core --test development_trial` checks retained failures/missing slots, substitutions, pairing, stable arms, local acceptance and malformed plans. These are local contract checks only.
