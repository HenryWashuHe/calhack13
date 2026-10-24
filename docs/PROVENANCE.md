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

# Constitution audit

Under the scrubber, a strip re-checks the snapshot on screen against eight
rules the kernel enforces when it commits changes:

1. Every open duty has a live owner.
2. Each open duty has one current holder: exactly one active assignment, held by its owner at the current epoch.
3. Requirements follow their duty's owner.
4. The budget held by live roles stays within the constitution.
5. Live roles hold only scopes the constitution grants.
6. Role and contract counts stay within the constitution's limits.
7. Reporting lines form a tree of live roles.
8. Delegated executors are live roles or known contracts.

The strip follows the scrubber and replay, so each frame is audited. **Audit
all revisions** fetches every stored snapshot (r0 to the current revision),
audits each one, and shows a cell per revision. Selecting a cell opens that
revision.

![Constitution audit across all revisions](img/console-audit.png)

The audit is a second, independent reading of the kernel's own stored
snapshots in `lib/audit.ts`. It is not the enforcement point. A red cell
means the stored state and the rules disagree, and should be investigated.
Every revision of both dev-scenario organizations passes (Org A r0–r10, Org B
r0–r5). Unit tests mutate a consistent snapshot to show that each rule fires.
