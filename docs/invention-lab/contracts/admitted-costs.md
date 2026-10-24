# Admission-aware method cost assessment

`genesis_forge::method_admitted_costs::assess_admitted_costs` joins one trusted
[admission journal](generation-admission.md) to one host-declared
[`CostScope`](accounting.md) and its optional trial evidence. It is read-only;
it neither authorizes nor executes a generation, trial or scoring request.

The scope must retain the journal's **entire original roster**, in its original
order, including organization, operation, plan, saved request and Prepared
checkpoint hashes. A selected-success subset, reordered roster or result hash
in place of its original identity is rejected. Each supplied later snapshot
must match the exact original history. An advanced snapshot additionally needs
a reservation; a journal entry alone does not prove a provider call occurred.

After validation, only the observed checkpoints' hashes are substituted in a
copy of the scope. The existing cost assessor checks the trial, slots, generated
package references and dependencies and retains its existing usage rules. A
method reused by several slots has one generation cost row. Unlinked inherited
or control methods remain allowed. Failed, rejected, missing and unused attempts
cannot disappear. Reserved-but-Prepared or missing observations remain unknown;
unreserved unused attempts are also unknown, not asserted to be free.

The source-free `genesis-admitted-method-cost-report/1` report includes:

- `declared_scope_sha256`: original scope with Prepared identities.
- `admission`: journal/roster hashes and exact reserved Prepared identities.
- `generation`: the full original roster assessment against its own grant.
- `costs`: combined generation/execution/scoring assessment, including the
  derived snapshot scope hash and exact observed evidence hashes.

The two grants remain separate. A larger combined grant cannot erase a known
original generation-grant overrun. Missing evidence and unknown usage do not
erase measured lower bounds or overruns. Reservations are requested capacity,
not measured usage. No acceptance, budget-compliance or benefit verdict follows
from successful report construction.

This binds a **host-declared combined scope**, not a pre-generation allocation.
The trial's exact method identity may only become known after generation. The
host retains supporting evidence, the original scope and trusted journal; this
join does not establish chronology, exhaustive history or protection against
copied/rolled-back journals. It adds no resource admission or metering guarantee.

It joins one roster to one trial. Adaptive B and return-to-A stages need their
own declarations when exact inputs exist. Adding their totals together does not
yet establish deduplicated full-history accounting, fair treatment allocation,
a fixed-budget comparison or a completed A → B → A loop. Provider, fixture and
unknown evidence labels remain unchanged.
