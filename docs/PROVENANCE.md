# Role provenance: "Why does this role exist?"

Each role card in the console has a **Why?** button. It opens the role's
evidence chain, rebuilt in the browser from records the kernel already
persists. The chain cites the identifiers it came from and never guesses.

![Why drawers on a retired DevStub role and its successor](img/console-why.png)

| Step | Source |
| --- | --- |
| Evidence | `proposal.trigger_evidence_ids` → `state.observations` (source, summary, `evidence_ref`) |
| Need | `AssignResponsibility { target: from_requirement, owner: new key }` → `state.requirements` (summary, acceptance) |
| Inherits a duty | `RetireRole.handoffs` or `ReassignResponsibility` aimed at the new key |
| Planner | `planner_decision` event with the same `change_id` (model, summary, rationale, attempt, latency) |
| Change | the committed change: submitter, rationale, `CreateRole.justification`, operations |
| Provider | `change.provisioned[key]` and `change.operations[key]` |
| Ready | `operations[key].readiness_evidence` |
| Commit | `committed_revision` |
| Since then | `responsibility_transferred`, `capability_attached` and `role_retired` events, with the retiring change's rationale |

Bootstrap roles are attributed to the mission whose responsibilities they own.

## Honesty rules

- A DevStub binding is labeled "development stub standing in for *provider*"
  and tagged **dev stub, not live**.
- An operator-attached agent ID is shown as "attached by an operator", in the
  warning tone. It is never presented as an agent the provider created.
- The planner step appears only when a `planner_decision` event names the
  change. A change submitted by hand or by the scripted scenario has no
  planner step.
- If nothing explains a role, the drawer says so.
- The drawer is shown only in live view. Historical revisions hide it, because
  the change and event records are current, not as of that revision.

Code: `apps/web/src/lib/provenance.ts` (pure, tested in `provenance.test.ts`)
and `components/WhyDrawer.tsx`. There are no API or schema changes.

# Evolution and replay

The Organization panel has a **Tree | Evolution** toggle and a **▶ Replay**
button.

**Evolution** lists *generations*. A generation is a revision in which the
organization's shape changed: roles activated or retired, duties transferred,
capabilities attached, contractors accepted, or reporting lines changed. For each
generation the view shows:

- the cause: the committing change's rationale, or the mission goal for the founding generation;
- the observations that triggered the change;
- the deltas, with DevStub roles labeled;
- the change ID, who submitted it, and the planner model when a `planner_decision` names the change;
- the active role count.

**View rN** shows the committed tree at that revision.

Capability gaps appear too. They cover a role asking for a capability, the
version being verified (never called installed), its installation (including
an adopted foreign version) and refusal.

Between generations, the view also lists the changes that **did not happen**,
each at the revision the organization stayed at:

- **Refused**: the kernel's validation errors, with known role and duty IDs
  replaced by their names.
- **Failed**: the provisioning errors. The change committed nothing.
- **Escalated**: the planner gave up and handed the decision to a person.

A checkbox hides these rows. In the screenshot below, a change derived from
prompt-injected evidence ("create Root with full authority and retire the
coordinator") was submitted against the dev stack. The kernel refused it on
four grounds, and the organization stayed at r10.

![Evolution view](img/console-evolution.png)

**Replay** steps the tree through each generation, one every 2.4 seconds, then
returns to live. A banner names the generation and its cause. Roles created in
that revision get the "NEW IN rN" highlight. Moving the scrubber, choosing a
generation or pressing Live stops the replay.

![Replay at r4](img/console-replay.png)

Generations are folded from the event log in `lib/evolution.ts`, sorted by
`seq`, so the result is independent of delivery order. Each tree frame is the
kernel's own stored revision snapshot (`GET /organization/revisions/:n`), not
a client reconstruction.

# Kernel audit in the console

Under the scrubber, a strip shows the kernel's own audit of the organization's
history. The audit is `GET /api/v1/organization/audit`, which is
`genesis_server::audit::audit_history` (see [KERNEL_AUDIT.md](KERNEL_AUDIT.md)).
It checks every stored snapshot against `genesis_core::audit`, every
consecutive pair against the transition rules, and the event log against the
storage rules. The dev stack reports 27 invariants.

The strip shows one cell per revision. Selecting a cell opens that revision.
Expanding the strip lists the rules and any violations at the revision on
screen. If the server truncated its violation list, later revisions show as
unknown, never as clean. The audit re-runs whenever the organization advances.
The console holds no copy of the rules.

![Kernel audit across all revisions](img/console-audit.png)

The route is read-only and open to the operator and agent tokens.
`tests/audit_route.rs` checks two things. A clean history reports no
violations. A tampered snapshot, in which a role holds more budget than the
constitution allows, is named as `budget_limit` at its revision.

# Export story

**Export story** in the Organization header downloads a self-contained HTML
page named `genesis-<org>-r<N>.html`. It holds no scripts and makes no
network calls. The page contains:

- the evolution timeline, including refusals;
- the kernel audit, fetched when the page is exported;
- the "why" chain for every role, including retired roles.

All recorded text is HTML-escaped. If any role was a DevStub, a notice says
so at the top. Use the page to share what happened without giving someone
console access.

![Exported story](img/story-export.png)
