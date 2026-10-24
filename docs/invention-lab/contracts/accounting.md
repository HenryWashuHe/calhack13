# Declared method cost accounting (experimental)

`genesis_forge::method_accounting::assess_costs` assesses a trusted `CostScope`:
up to 64 generation operations and one optional complete `TrialPlan` (up to 64
slots, in declared order). The scope pins each generation's organization,
operation, plan hash, **saved** request hash and exact retained checkpoint hash.
It pins the trial reference and all slot identities separately. The host retains
the scope and the referenced evidence; the report binds their declared scope by
hash. No mutable alias resolution or provider request occurs here.

Generation operations are counted once, independently of whether they produced
source, were used, or supplied a method reused by several slots. Rejected, failed,
interrupted, prepared, pending and missing generation remain visible. Explicit
`generated_links` check exact package references and dependencies against trial
slots; cross-organization use and different public task documents are allowed.
`matched_package_links` does not authenticate lineage or prove execution/behavior.
Failed or missing generation and a missing trial leave links unmatched.

The `genesis-method-cost-report/1` report retains stage rows, exact evidence hashes,
outcomes, generation endpoint provenance and separate generation/execution/scoring
and combined totals. Every resource reports a known lower bound, unknown flag,
overflow flag and known-over-limit flag. Overflow saturates the lower bound at
`u64::MAX` and always marks the limit exceeded, even at a `u64::MAX` grant. Unknown
components do not erase known spend or known overruns. A false
`individual_limit_exceeded` means **no observed overrun**, not known compliance.
Stage totals' limit flags use the scope's combined ceiling; execution/scoring rows
also retain their individual grant overruns. Missing scoring is unknown even
when execution produced no output. Absence never proves that work was free.

These are accounting-only identity checks. The existing execution, scoring and
acceptance validators still reject over-budget evidence. Reported model usage is
provider/host-observed, and a fixture or `unobserved` label remains such. Summed
recorded wall time is stage duration, not elapsed pipeline time or independently
metered total work. Generation records contain no simulation action, so completed
generation contributes zero simulation steps; other missing usage remains unknown.

The host must decide and declare historical/inherited cost inclusion, complete
operation history, trustworthy storage, budget timing and admission enforcement.
Successful generation can precede a trial whose method hash did not yet exist.
This report does not prove that its roster was complete or frozen before spending,
and does not measure dependency development, setup, provider internals or omitted
orchestration overhead. It is not a monetary bill, acceptance verdict, enforced
combined budget or G4 fixed-settings/improvement result. No CLI integration or
change to Henry's executor/scoring runtime is included in this library increment.
