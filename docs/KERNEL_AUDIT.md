# Kernel audit

GENESIS checks its own history. `genesis_core::audit` states the structural
invariants of an organization independently of the validator that is meant to
enforce them, and `genesis audit` applies them to a stored organization.

```bash
genesis audit --db data/dev/org_a.db --org-id org_a   # JSON report, exit 1 on any violation
```

The database is opened read-only and read in one transaction, so a running
server can stay up and the report still describes one consistent history.

## What is checked

**Every state** (`audit`):

| Invariant | Meaning |
| --- | --- |
| `role_limit` | live roles ≤ `max_active_roles` |
| `budget_limit` | budgets held by live roles ≤ the constitution budget (no overflow) |
| `constitution_scopes` | every role's scopes are constitution scopes |
| `role_revisions` | no role from a future revision; a retired role records a valid retirement revision |
| `reporting_tree` | reporting edges name known roles, no self-report, no cycle |
| `assignment_references` | assignments name known roles and responsibilities |
| `single_active_assignment` | at most one active assignment per responsibility |
| `active_assignment_current` | an active assignment is on an open responsibility, at its current epoch, for a live role |
| `live_owner` | an open responsibility owned by a role is owned by a live role |
| `executor_exists` | executors name known roles or contracts |
| `contract_limit` | open contracts ≤ `max_active_contracts` |
| `gap_references` | gaps name their mission, requirement, role and responsibility, at a past revision |
| `installed_gap_has_installation` | an `installed` gap names an accepted installation of exactly that capability version |
| `live_gap_limit` | live gaps ≤ 16 |
| `requirement_evidence` | requirements cite recorded observations |
| `map_key_identity` | every entity is stored under its own ID |

**Every later state** (`audit_transition`, also across several commits):

| Invariant | Meaning |
| --- | --- |
| `revision_advances` | the revision strictly increases |
| `history_is_append_only` | no mission, role, assignment, responsibility, requirement, contract, installation, observation or gap disappears |
| `observations_immutable` | an observation never changes |
| `lifecycle_reachable` | every lifecycle moves only along its transition table; a gap never moves back, keeps its verified capability version through installation, sets a publication reference at most once, and never changes its final outcome |
| `epoch_monotonic` | responsibility epochs never decrease; an assignment is never rebound |
| `gap_binding_stable` | a gap keeps its whole frozen binding: ID, mission, requirement and digest, triggering evidence, role, responsibility, capability, purpose, sources, opening revision and claimed acquisition |

**Storage** (`genesis audit` only): each snapshot row holds its own revision
and organization, revisions are contiguous, and every event names this
organization and a stored revision.

## Refusing ill-formed commits

With `GENESIS_AUDIT_COMMITS=1`, every `Store::commit_state` audits the new
state and the step from the stored previous revision inside the same
transaction, and refuses the commit (`InvariantViolation`, HTTP 500, nothing
written) if anything breaks. Off by default; recommended for `genesis dev` and
the demo. `Store::with_commit_audit` sets it per handle.

Measured on this branch with it on: all server and workspace tests' real
commits pass. The only refusals came from test fixtures that tamper with state
on purpose (setting a role back to `Active`, an installation back to
`Checking`, retiring without a revision), which is what they simulate. A full
`genesis dev` + dev-scenario run commits all 12 steps with zero refusals.

## Where it runs

- `crates/genesis-core/tests/engine_fuzz.rs`: 8 seeds × 1000 random steps
  (untrusted proposals plus gap, contract, completion and installation steps);
  every resulting state and transition is audited, a coverage check fails if a
  step kind is never reached, and self-tests prove each invariant fires on a
  tampered state.
- `crates/genesis-server/tests/history_audit.rs`: a stored history passes;
  rewritten snapshots, a missing revision, an orphaned event and the wrong
  organization are each named.
- After the demo: see [DEMO_RUNBOOK.md](DEMO_RUNBOOK.md).

## What it does not prove

A clean audit means every stored state is well formed and every step between
them was a legal move. It does not show that the organization did useful work,
that a provider really acted, or that evidence behind an observation is true.
