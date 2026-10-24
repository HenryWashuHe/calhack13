# GENESIS technical specification

**Canonical framing: networked recursive capability improvement with fixed model weights.**

Status: baseline architecture drafted October 21, 2026; direction revised October
23. Section 14 now states the governing design direction. The original layouts,
routes and venue workload below are design history, not a current inventory or
product completion gate. Use [implementation status](IMPLEMENTATION_STATUS.md)
for implemented vs unmerged vs proposed behavior, the [invention-lab proposal](INVENTION_LAB_DEMO.md)
for the new demonstration, and the [handoff](AGENT_HANDOFF.md) for ownership.

The network should retain executable capabilities and methods for developing
further capabilities across independent organizations. Organizational evolution
changes accountable roles and specialists; capability evolution changes what
they can execute and how they develop it. Model weights stay fixed. Measured
recursive benefit remains unproven. Provider contracts and authenticated-live
evidence are tracked separately in [PROVIDER_CONTRACTS.md](PROVIDER_CONTRACTS.md)
and [LIVE_GATES.md](LIVE_GATES.md).

## 1. Stack and boundaries

The organization kernel is **Rust** using Axum for HTTP and Tokio for asynchronous execution. Use serde for typed JSON envelopes, reqwest for provider HTTP calls, and SQLite for authoritative state. The UI is TypeScript/React. Apify source generation targets TypeScript within a fixed, versioned scaffold. A small Python sidecar runs the official Fetch uAgents SDK; it is a transport and external-agent bridge, not a second organization controller.

Axum is designed around Tokio and the Tower middleware ecosystem. See [Axum documentation](https://docs.rs/axum/latest/axum/) and [Tokio](https://tokio.rs/). Pin dependency versions and commit application lockfiles during implementation.

Rust provides useful type and memory safety. Operational robustness additionally depends on durable state, explicit authority, bounded concurrency, reconciliation, and the acceptance tests below.

Proposed source layout, to be created during implementation:

```text
crates/genesis-core/       domain types, authority and mutation validation
crates/genesis-server/     Axum API, scheduler, persistence, event stream
crates/genesis-providers/  Elastic, Apify, CREAO and Fetch-sidecar adapters
apps/web/                 TypeScript/React mission and evidence UI
services/fetch-bridge/    Python uAgents transport and independent specialists
actors/scaffold/          TypeScript Actor build scaffold
fixtures/                 public demo portal fixtures and private oracle fixtures
docs/                     specifications and research
```

For the demo, run the same Rust service in two organization instances with separate state stores and conversations. A shared capability service provides manifest publication, discovery, and execution brokerage. These may share code and a host. Do not create a service per conceptual box or introduce Kafka/Redis solely for the prototype.

## 2. Historical venue integration workload (superseded)

This scenario is retained solely to explain existing code and integration fixtures. Event organization is no longer the product or chosen demonstration. The following requirements belong to that historical fixture; they do not gate the new [invention-lab proposal](INVENTION_LAB_DEMO.md). Section 14 governs selective transfer and recursive development.

Organization A operates a simulated campus event for 120 attendees. Its initial responsibilities cover mission coordination, program scheduling, and attendee information. A venue closure creates relocation and transport coordination work while program work continues.

The historical fixture was designed to:

1. Derive the unmet requirements from the observation.
2. Explain why the existing organization needs new ownership and a new web capability.
3. Create an Elastic relocation specialist and assign meaningful responsibility.
4. Discover and recruit a separately running Fetch logistics specialist.
5. Generate, build, and independently check a new Apify venue-availability Actor.
6. Use the capability to establish a feasible plan in the simulated operations ledger.
7. Create a CREAO venue-monitor agent and transfer continuing monitoring to it.
8. Run that same monitor on a later event, without recreating it.
9. Let Organization B discover and install the approved capability using its own state and different input.
10. Retire A's temporary relocation unit without losing unfinished commitments or the shared capability.

The new role names, charters, responsibility changes, selected specialist, and parser source must be derived from the requirement. The mutation vocabulary, generic runtime, and Actor scaffold may be predefined. A new URL alone must not force creation of a new role.

## 3. Domain model

Use separate Rust newtypes for IDs and explicit enums for state. Provider identifiers never replace domain role or assignment identifiers.

| Entity | Required fields and meaning |
|---|---|
| Organization | `org_id`, `revision`, constitution/limits, private store, status |
| Mission | `mission_id`, goal, constraints, deadline, success checks, status |
| Observation | `observation_id`, mission, source, observed time, evidence reference |
| Requirement | `requirement_id`, triggering evidence, acceptance predicate, status |
| Responsibility | `responsibility_id`, purpose, accountable owner, trigger, lifecycle/termination condition |
| Role | `role_id`, charter, authority scope, required capabilities, reporting edges, lifecycle |
| Agent binding | Provider, provider agent/address ID, effective configuration digest, conversation reference |
| Assignment | `assignment_id`, role, binding, monotonically increasing epoch, lease, state |
| Contractor agreement | Specialist address, protocol, accepted task scope, deadline, contract ID, state |
| Capability version | ID/version, source digest, pinned artifact, schemas, applicability, verification evidence |
| Installation | Org-local binding of a capability version, configuration, acceptance result and grants |
| Job/attempt | Domain task, attempt ID, provider IDs, raw status, normalized status, deadlines, result references |
| Organization change | Expected revision, evidence, typed operations, provisioning steps, status |
| Event/outbox/inbox | Append-only event IDs, pending deliveries and deduplicated receipts |

Persist authoritative records in SQLite. Maintain searchable projections in Elastic. Delayed indexing must not change the authoritative organizational state. Capability discovery returns candidates; installation rechecks the canonical manifest and verification status.

## 4. Invariants

- Every accepted responsibility has exactly one accountable owner; delegation does not silently transfer accountability.
- An unmet requirement remains owned by the mission controller until assigned.
- Only an active assignment with a current epoch and valid grant can dispatch privileged work.
- Capability bindings reference exact approved versions, never a mutable `latest` reference.
- A role cannot enlarge its own authority beyond the organization constitution.
- A retirement cannot discard open obligations.
- Org B cannot inherit Org A's private state, credentials, conversations, or connector identifiers.
- Provider success and mission success are separate states.
- Delivery is at least once; state-changing effects must be idempotent or reconciled. Do not promise global exactly-once execution across sponsor APIs.

## 5. Organization-change protocol

An Elastic mission agent produces a structured proposal. Treat it as untrusted input and validate it in the Rust kernel. A proposal has `change_id`, `expected_org_revision`, `trigger_evidence_ids`, rationale, desired responsibility changes, role changes, capability requirements, and budget/lease limits.

Supported operations are `CreateRole`, `RecruitSpecialist`, `AssignResponsibility`, `DelegateTask`, `ReassignResponsibility`, `AttachCapability`, `ChangeReportingEdge`, and `RetireRole`. The kernel validates referential integrity, unique accountability, authority, cycles where prohibited, and resource limits.

Create a role only when there is a distinct continuing responsibility and a reason for separate execution: concurrency, authority, independent ownership, expertise, or lifecycle. Otherwise reuse an existing role, attach a capability, correct inputs, retry a transient failure, or escalate.

```text
PROPOSED -> VALIDATED -> PROVISIONING -> READY -> COMMITTED
                   \-> REJECTED          \-> FAILED
```

External provisioning is a saga, not a cross-provider database transaction. Persist each requested operation and returned resource ID before advancing. Newly provisioned agents remain disconnected from mission triggers and grants until activation.

When readiness checks pass, commit one local transaction that increments the organization revision, assigns ownership, advances assignment epochs, records capability bindings, and appends initial dispatches to the outbox. A revision conflict requires reevaluation against current state. If provisioning fails, existing accountability remains intact; retain or clean up unused provider resources separately.

Role lifecycle:

```text
PROPOSED -> PROVISIONING -> READY -> ACTIVE -> DRAINING -> RETIRED
                      \-> PROVISIONING_FAILED
```

`DRAINING` blocks new assignments. Finish or transfer open work, stop obsolete timers, revoke execution grants, and fence old epochs before marking retired. Provider deletion is optional cleanup. Releasing a Fetch contract must not stop the contractor's work for another organization.

## 6. Jobs, events, and recovery

Every dispatch includes org, mission, role, assignment, assignment epoch, task, attempt, correlation ID, idempotency key, deadline, and capability version where relevant. The authenticated caller determines organization identity; do not trust a supplied `org_id` alone.

Normalize jobs to `QUEUED`, `SUBMITTING`, `RUNNING`, `VERIFYING`, `SUCCEEDED`, `FAILED`, `RESULT_REJECTED`, or `UNKNOWN`. Preserve provider-native statuses and responses for diagnosis. A cancellation request is not proof the remote process stopped.

Persist scheduled due times, leases, attempts, and outbox items. A crashed worker can recover expired leases. Duplicate delivery is expected; the simulated commitment ledger enforces uniqueness on a logical operation ID.

On a create timeout, reconcile using known provider IDs or inventories before resubmitting. Do not assume provider idempotency. A lost SSE connection may leave an execution outcome unknown. Late results are archived, but an obsolete assignment epoch cannot change current commitments.

Use bounded provider queues, request timeouts, and backoff. One CREAO account-wide limiter must cover all creates, polls, and runs. Separate model output parsing from provider completion and domain verification. Allow a bounded correction attempt for malformed output; never execute arbitrary text as a command.

## 7. Provider adapters

All interfaces in this section are GENESIS abstractions. Exact provider routes and limitations are in the provider-contract document.

| Adapter | Operations | Responsibility |
|---|---|---|
| Elastic | Ensure agent/skill/retrieval tool; invoke; fetch configuration | Internal reasoning and evidence retrieval |
| Fetch bridge | Discover; request offer; accept; dispatch; release | Independent identity, contracting and typed messages |
| Apify | Create Actor; create version; build; poll; run pinned build; fetch dataset | New executable web capability |
| CREAO | Create agent; poll creation; patch instructions; invoke; poll result; access memory | Reusable continuing operational role |

Use Rust HTTP adapters for Elastic, Apify, and CREAO. The Fetch sidecar accepts typed local HTTP requests and emits authenticated callbacks/messages to the kernel. It cannot authorize organizational changes. Validate sender address, protocol, contract, task, and attempt on incoming Fetch results. A transport receipt does not mean a task was accepted.

For Elastic's first integration, use native `index_search` tools and validated JSON command envelopes in direct conversation responses. The host executes approved commands and returns results. External MCP connector support is optional until its configuration is smoke-tested. Do not depend on interactive subagent approval prompts or experimental durable execution features.

CREAO gets a generic bridge skill with a supporting HTTP script. A dynamically created role receives its own charter and exact capability binding. GENESIS supplies its schedule/event triggers; native CREAO schedule creation is not assumed. Require a bridge-readiness run because skill installation may precede sandbox file availability.

Each CREAO execution receives an expiring job grant restricted to organization, role, assignment epoch, job, capability version and permitted operation. The gateway checks active authority on every call. The role never receives the Apify creation token.

## 8. Capability generation and verification

The capability request includes a goal, input/output schemas, allowed public source, applicability conditions, and acceptance criteria. Generate only a narrow extraction module inside the TypeScript Actor scaffold. Do not generate the kernel, verification policy, or credentials logic.

```text
REQUESTED -> SYNTHESIZING -> BUILDING -> TESTING -> APPROVED -> PUBLISHED
                                      \-> REJECTED
```

Build success and input-schema validity do not establish extraction correctness. Independent checks compare Actor output with canonical fixture truth held outside the generated Actor and outside the public portal. Include two successful fixture variants and at least one explicit unsupported/missing-data case. Check IDs, capacities, prices, time windows, availability, and source evidence. Accessibility claims in the demo mean fixture-declared attributes only.

Permit at most one automated repair in the initial MVP. A repair creates a distinct version and reruns the same acceptance suite. Failed candidates cannot be discovered as usable or attached to active roles.

Persist source-file digest, Actor ID, exact build number, build ID, test report, run IDs, and datasets. An optional provider image digest is additional evidence, not a required field until verified live. Verification records state the tested environment and limitations; there is no universal `safe=true` label.

The forge token remains in the trusted backend. Generated runs use limited Actor permissions plus timeout, memory, and cost limits. These do not establish arbitrary-code containment or an egress allowlist. The prototype uses public synthetic data and no sensitive credentials. Stronger isolation is a production requirement, not a demo claim.

## 9. Cross-organization inheritance

A published manifest contains:

```text
capability_id, version, publisher, source_digest
intent, applicability_conditions, known_limitations
input_schema, output_schema, required_permissions
actor_id, exact_build_number, build_id
role_template, responsibility_template, workflow_procedure
local_configuration_schema
acceptance_suite_ref, verification_report_ref, provenance
```

The publisher selects shareable content. Raw private traces and account credentials are excluded. Publication follows verification; an agent cannot self-approve it.

Org B searches by its unmet requirement through Elastic, fetches the canonical manifest, checks compatibility and local policy, runs local acceptance, and records an installation. B instantiates its own agent/role binding and grants. Compatible adoption must invoke the approved artifact with different input and zero new source-generation calls. An incompatible environment requires rejection or a separately verified adapted version.

For the prototype, independent organizations have separate runtime instances, private state, conversations, task queues, and grant identities. Use separate provider accounts/permissions where possible. Two CREAO workspaces alone do not isolate account-wide secrets. If public demo data share an account, label that as logical organization separation, not production tenant isolation.

## 10. Proposed application API

These are planned GENESIS routes, not sponsor APIs. Handlers derive the caller's permissions from authentication. Management routes are kernel/operator-only; agents submit proposals under restricted identities.

| Route | Contract |
|---|---|
| `POST /api/v1/missions` | Create mission and initial ownership |
| `POST /api/v1/missions/{id}/observations` | Submit evidence, without specifying the role to create |
| `GET /api/v1/organization` | Return current revision, roles, assignments and responsibilities |
| `POST /api/v1/org-changes` | Submit schema-validated mutation proposal |
| `GET /api/v1/org-changes/{id}` | Return validation/provisioning/commit status |
| `GET /api/v1/jobs/{id}` | Normalized status, provider references and result scope |
| `GET /api/v1/events` | SSE stream with event IDs and reconnect cursor |
| `POST /api/v1/capabilities/search` | Search approved, authorized candidates |
| `GET /api/v1/capabilities/{id}/versions/{version}` | Immutable canonical manifest |
| `POST /api/v1/capability-installations` | Start local compatibility and acceptance checks |
| `POST /api/v1/capability-runs` | Execute under a checked job grant |

Mutation and execution submissions accept a client request ID for deduplication. Organization proposals include the expected revision. Return explicit errors for revision conflict, missing authority, incompatible capability, verification failure, provider configuration failure and unknown external outcome.

Keep one set of Rust domain structs as the contract source. Export or maintain matching JSON schemas for the TypeScript UI and Python bridge; add contract tests to detect drift.

## 11. UI and observable evidence

The interface has three synchronized views: mission constraints/results, organization/ownership, and execution evidence. It reads real persisted events rather than manufacturing graph animation.

Display provider IDs, organization revision, role/assignment state, responsibility transfers, verification results, and capability lineage. Distinguish pending build, provider completion, accepted result and mission success. Org B's view shows its own inputs, installation receipt, artifact digest and zero source-generation calls.

A timer/event runs the existing CREAO monitor a second time. Retirement visibly stops dispatch and removes authority while preserving the published capability. A graph node appears as provisioning before activation; failed provisioning cannot look like a working employee.

## 12. Acceptance tests

| Test | Pass condition |
|---|---|
| Requirement causality | Observation precedes and is referenced by the mutation; new IDs/artifact were absent initially |
| Role vs tool choice | Format-only change can update a capability; new continuing obligation justifies a new role |
| Meaningful new ownership | New role owns deliverables, receives subsequent events and has distinct bounded authority |
| Partial provisioning | Failure/restart never activates an unready role or drops existing accountability |
| Revision conflict | Concurrent proposals cannot create two accountable owners or overwrite a committed revision |
| Verification gate | Incorrect held-out extraction remains unavailable despite a successful build |
| Handoff fencing | Old-epoch results cannot modify commitments after a transfer |
| Durable timers | Crash and duplicate delivery produce one logical ledger effect and monitoring resumes |
| Independent adoption | B discovers and locally accepts the same artifact, uses different input, and performs no synthesis |
| Isolation/compatibility | A private canary is absent from B; incompatible adoption is rejected or separately adapted |
| Retirement | Open work is resolved/transferred, obsolete timers stop, and stale grants fail |
| Baselines | Report correctness, missed obligations, human intervention, latency and cost for fixed-org and no-inheritance modes |

Test critical domain transitions and effects, not merely enum serialization. Include a strong generalist/fixed-organization comparison with comparable tools and budget. Demonstrating new ability does not alone prove that more agents are better.

## 13. Integration gates and scope

The following records the original integration plan and hackathon scope. For current validation status and active work, use [live gates](LIVE_GATES.md) and the [handoff](AGENT_HANDOFF.md).

The original plan called for proving authenticated Elastic create/invoke/retrieve; Fetch discovery and offer/accept/result messaging; Apify create/build/run/results; and CREAO create/run/custom-skill HTTP execution before feature work, measuring actual cold latencies and verifying account limits. Those checks were untested when this baseline was drafted; this paragraph is not a current test report.

The original, now-superseded demo scope was one complete mutation, recruitment, Actor creation and verification, persistent CREAO role with a second run, B's adoption, and retirement/handoff. One repair was optional in that original plan. Defer real payments, real reservations, public marketplaces, unrestricted spawning, automated credential discovery, and production multi-tenancy.

A three-minute all-cold path is a target, not a guarantee. If measured provider latency exceeds it, use an honestly labeled full-run recording alongside live progress and reuse. Never display prebuilt resources as freshly synthesized. Team ownership and the hour-by-hour integration sequence are in [TEAM_WORKSTREAMS.md](TEAM_WORKSTREAMS.md).

## 14. Selective transfer and recursive capability improvement

This section is the governing design direction, not newly implemented behavior. The detailed [goal, milestones, and evaluation protocol](RECURSIVE_CAPABILITY_IMPROVEMENT.md) and [proposed invention-lab demonstration](INVENTION_LAB_DEMO.md) supersede the venue product narrative. Runtime contracts remain unchanged by this documentation update; the [handoff](AGENT_HANDOFF.md) assigns bounded work.

The architecture is general when it can admit different capability types, select them for their applicable tasks, and apply the appropriate verification method. It does not require every artifact to transfer across domains or improve the immediately following task. Fixed model weights permit system-level adaptation through retained executable capabilities; execution tasks are not weight-training steps.

Required design properties:

- Distinguish accumulation, reuse/composition, and measured improvement of capability creation, verification, or repair. Capability count alone demonstrates only accumulation.
- Preserve task-specific applicability, schemas, permissions, environment assumptions, known failures, exact versions, provenance, and verification scope. Extend the existing manifest rather than creating a second competing capability catalog.
- Treat discovery as candidate retrieval. Receiving organizations make their own adoption decisions; publisher verification does not prove target-task suitability. Insufficient evidence and operational uncertainty remain explicit.
- Permit non-reuse. No candidate, incompatible candidate, insufficient evidence, and unavailable discovery are different outcomes. Preserve the existing rule that an ambiguous search or failed adoption cannot silently authorize synthesis.
- Make executable builder/test/repair workflows and pinned dependencies explicit before claiming recursive composition. A workflow description is not an executable package. Inherited workflows cannot rewrite the kernel's authority rules or approve their own evaluation.
- Associate failures and replacements with exact versions. Deprecation or quarantine should filter discovery while preserving audit history and installations needed for reconciliation; adoption of a repair remains local and independently checked.
- Evaluate direct reuse, related transfer, and appropriate non-reuse separately. Compare a fixed baseline, accumulated task capabilities, and accumulated development workflows under the same model and declared budgets. Include acquisition and selection costs, task failures, and family-specific regressions.

These mechanisms constrain adoption and make evidence inspectable. They do not guarantee universal transfer, zero negative transfer, or monotonically improving performance. A positive recursive-improvement claim requires an inherited development capability to improve later development outcomes on independent tasks; an honestly reported null or negative result must not be relabeled as success.
