# Stage allocation and later trial binding

`method_stage::StageAllocation::prepare` creates a source-free allocation from
one exact generation-admission roster and `StageLimits`. It requires every
original snapshot, each still Prepared, and zero reservations. The caller must
hold the admission lock and persist the allocation before participating work.
Checking supplied snapshots alone does not establish trusted chronology.

For each of the five resource dimensions, checked arithmetic requires:

```text
full generation grant
  + max_trial_slots × (per-slot development grant + per-slot scoring grant)
  ≤ total stage grant
```

Overflow is an error. Slots are bounded to 1–64 and count every candidate,
control and repetition, not just successful treatments. Grants are not multiplied
again by instance count: they already cover each slot's entire execution/scoring.
The full generation grant is included even when attempts fail or remain unused.
Unused capacity does not increase another grant or erase unknown usage.

After the generated method identity becomes available, `allocation.bind(trial,
scope)` constructs a `StageTrialBinding`. It pins the allocation, full exact trial
reference and original cost-scope hash. The trial must use the exact per-slot
grants, no more than the allocated slot count, the entire original generation
roster, exact ordered trial slots and the exact total ceiling. Imported/control
methods and non-reuse remain legal; no successful generated-method link is forced.

A restored binding must pass `validate_against` before use. Its `assess` method
also validates the admission roster and delegates to the existing
[admitted-cost assessment](admitted-costs.md). It retains separate generation and
combined costs, missing/failed/unknown observations, lower bounds and overruns.
The report pins allocation and binding hashes. It is not an acceptance or budget
compliance verdict. Package links are checked with supplied generation evidence
by the existing assessor, not proved by the initial binding alone.

These are structural contracts. A pure bind function can construct multiple
individually valid plans under one allocation. The participating host must retain
ONE create-only binding per allocation and validate it before any runtime intent.
The allocation must likewise precede generation intent. Runtime entry checks and
actual metering remain with the executor/scorer owner. No library call supplies
storage, authority, process isolation or new enforcement of time/tokens/billing.

Copies, rollback, alternative paths and supplied stale snapshots remain outside
the trusted-storage boundary. Hashes prove no timing, writer authentication or
complete history. A whole-chain fixed-budget claim additionally needs a finite
stage-ceiling list declared before A begins; individually allocated adaptive
stages cannot establish that retrospectively. Fixed provider settings, fair
controls, a model-developed return chain and measured recursive benefit remain
separate open gates. No provider call or source execution occurs here.
