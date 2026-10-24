# Experimental development-call ledger

This additive G0/G1 library state machine is stacked on the trial-plan contract.
It does not run or sandbox a method. Construct `ExperimentLedger` from the host's
frozen `TrialPlan` and slot. Acceptance and comparison development both use the
plan's exact public instances; hidden scoring remains a separate trusted API.
Every transition and validation takes a host-supplied `ExperimentScope` with
that expected plan and slot, never the slot claimed by incoming evidence.

Reserve one positive step allowance and durably persist the pending record
before invoking the evaluator. Settle that exact call with the observed evaluator,
result and actual simulation steps, or explicit failure/unknown evidence. The
next reservation uses cumulative remaining steps; failed measured calls remain
charged. A pending call, unknown result or unknown metering blocks further calls.
No retry or restart is permission to resubmit, refund unknown usage or reset the
grant. Reconciliation can settle an existing pending call without resubmission.
Observed overruns remain in the record and block further calls; integer overflow
preserves per-call values with unknown total usage. Neither can validate as within limits.

Seal after stopping the candidate. Sealing can retain missing/unknown evidence;
it prevents further transitions and creates an exact ordered transcript reference.
Persist the exact UTF-8 envelope returned by `transcript_json` under that reference.
`validate_execution` checks that reference and simulation usage against the
execution receipt. It does not infer design quality or success from a return.

The host must enforce exclusive durable state, anti-rollback, trusted metering,
wall/model/token limits, candidate isolation and the absence of unrecorded
simulation paths. This in-memory state machine cannot establish those properties,
authenticate records or prove that an external process stopped before disclosure.
Zero-step failures can repeat until host wall-time or other admission limits stop
them; this module provides no additional experiment-count or memory bound.
