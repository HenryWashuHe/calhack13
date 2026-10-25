# Design catalogue audit (experimental)

`genesis_core::development_catalogue::DesignCatalogue` is a pure post-run audit
contract. It declares an ordered set of 1–256 `CatalogueEntry` pairs for one
caller-supplied [`ExperimentScope`](../../../crates/genesis-core/src/development_experiment.rs).
The record pins the full trial-plan reference, slot and expected execution-binding
hash. Its versioned canonical hash includes the declared order. Restored records
must be validated against the caller's independently retained plan and slot.

`new(scope, entries)` validates the declaration; `canonical_hash(scope)` supplies
its identity. Each entry explicitly declares an `experiment` reference and an
`output` reference with equal domain, schema, version and digest; IDs may differ.
This supports the runtime's `submission` and `output` labels without matching by
SHA alone or silently inferring aliases. Duplicate full experiment references or
duplicate full output references are rejected. Equal logical labels with different
digests remain distinct: existing design artifacts reuse the label `submission`.
The two references declare shared bytes; this library does not verify those bytes.

`audit(scope, optional_ledger, optional_execution)` reuses experiment accounting
and execution identity validation. It returns every supplied experiment record,
including its original index and pending, failed or unknown settlement, alongside
an optional catalogue index. An index of `None` reports an outside reference;
the record is never removed. The optional output receives the same check.

Missing and unsealed ledgers, absent receipts, outcome/usage uncertainty and
overruns remain explicit. For a sealed ledger plus receipt, the exact transcript
reference and simulation usage must agree before overruns can be retained as
findings. Foreign plan, slot, method, environment, transcript or usage bindings
are errors, even when a budget was exceeded. Unsealed or missing ledgers cannot
establish that transcript link.

`recorded_evidence_complete` only means the supplied records are sealed, linked,
settled and known within their grants. It is **not membership or success**:
outside designs and confirmed failures can have complete records. Consumers must
inspect membership, outcomes and accounting separately. The audit does not read
artifact bytes, check normalization, execute methods or change existing records.
It does not prove predeclaration timing, complete history, independent ownership,
runtime enforcement, access to method source, search order or a search policy.
Caller-retained hashes supply identity, not authentication or improvement evidence.
The PR116 retained B plan, ledger and receipt are covered by a parse-only test;
deriving a catalogue from those observations does not retroactively predeclare it.
