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

## CLI and retained files

```sh
cargo run -p genesis-forge -- method-stage-prepare \
  --manifest stage-input.json --admission admission.json --allocation stage.json
cargo run -p genesis-forge -- method-generate-step \
  --checkpoint generation.json --admission admission.json --allocation stage.json
cargo run -p genesis-forge -- method-stage-bind \
  --manifest trial-input.json --allocation stage.json
cargo run -p genesis-forge -- method-cost-assess \
  --manifest cost-input.json --admission admission.json --allocation stage.json
```

The stage manifest uses `schema: "genesis-stage-input/1"`, `limits` containing
all `StageLimits` fields above, and `prepared_checkpoints` listing every exact
Prepared snapshot path. Preparation takes the existing admission lock before
checking reservations/snapshots and saves the allocation create-only. An occupied
journal lock, started/reserved attempt, incomplete roster or invalid budget fails
without modifying the journal or snapshots. It performs no provider request.

The trial manifest uses `schema: "genesis-stage-trial-input/1"` and `plan`/`scope`
paths. Binding locks the allocation, validates the full trial/scope, then creates
**`stage.json.trial-binding.json`**. The suffix is appended to the allocation's
full pathname. There is no caller-selected binding output path. A second bind,
including a different individually valid plan, refuses to overwrite this file.
Keep one retained canonical allocation pathname; copies, hard links, alternate
paths and rollback remain outside the cooperating-path guarantee. The command
cannot detect work already started outside these entry points.

Paths inside either manifest resolve relative to that manifest; command-line
paths resolve relative to the working directory. Existing bounded regular-file
reads and create-only storage apply. No command removes a stale lock for you.

On the existing generation step, `--allocation` requires `--admission` and checks
the allocation's exact roster before credentials, reservations, recovery or
provider effects. Existing no-refund reservation/write ordering remains unchanged.
The option is deliberately optional: old commands without it retain their
previous behavior and do not establish stage allocation ordering.

On cost assessment, the option reads the companion binding, requires its exact
trial plan, validates all identities and emits `genesis-stage-cost-report/1`.
The usual [cost manifest](accounting-cli.md) still applies; evidence arrays may
be empty, but a bound-stage assessment needs its named trial plan. Missing,
malformed or substituted allocation/binding/evidence fails with no fallback.
This mode is read-only and retains unknown usage and overruns. Exit 0 means
report construction, not budget compliance. Use immutable snapshots: reading
several mutable files does not provide an atomic multi-writer view.

Henry's executor/scorer still needs to consume the retained binding before
runtime intent. This increment adds no runtime gate or whole-chain allocation.
