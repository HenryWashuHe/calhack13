# Kernel audit

GENESIS checks its own history. `genesis_core::audit` states the structural
invariants of an organization independently of the validator that is meant to
enforce them, and `genesis audit` applies them to a stored organization.

```bash
genesis audit --db data/dev/org_a.db --org-id org_a   # JSON report, exit 1 on any violation
```

The database is opened read-only; a running server can stay up.

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

**Every later state** (`audit_transition`, also across several commits):

| Invariant | Meaning |
| --- | --- |
| `revision_advances` | the revision strictly increases |
| `history_is_append_only` | no mission, role, assignment, responsibility, requirement, contract, installation, observation or gap disappears |
| `observations_immutable` | an observation never changes |
| `lifecycle_reachable` | every lifecycle moves only along its transition table; a gap never moves back and its final outcome never changes |
| `epoch_monotonic` | responsibility epochs never decrease; an assignment is never rebound |
| `gap_binding_stable` | a gap keeps its requirement, digest, capability, role and claimed acquisition |

**Storage** (`genesis audit` only): each snapshot row holds its own revision
and organization, revisions are contiguous, and every event names this
organization and a stored revision.

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
