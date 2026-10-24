# GENESIS team workstreams

> **Historical coordination plan — superseded.** The current framing is networked recursive capability improvement with fixed model weights. Event organization is no longer the product/demo. Use [the current ownership map](AGENT_HANDOFF.md#current-ownership-and-next-work) and issue #4; assignments and “must” statements below are historical, not current instructions.

Four-person plan for a 36–48 hour build. The demo must complete one cycle: a disrupted event creates a new responsibility and specialist role, recruits an independent agent, gains a verified executable capability, transfers continuing work, and lets a second independent organization inherit that capability.

**Stack:** Rust + Axum/Tokio for the organization kernel and HTTP integration adapters; TypeScript + React for the UI; TypeScript for generated Apify Actors. A small, isolated Python service uses the official Fetch uAgents SDK for registration and agent messaging. Use SQLite for authoritative demo state and Elastic for searchable projections, evidence, and approved capability manifests. Rust owns organizational authority, durable jobs, and state transitions; the Python service is a protocol bridge, not a second orchestrator. Language choices are implementation decisions, not sponsor requirements.

The [research report](../GENESIS-organization-evolution-research.md) records API sources and limitations. Its API contracts were researched but have not yet been proven with our accounts. The gates below are therefore the first deliverable.

## Ownership

For current agent assignments and the user-authorized Codex kernel fallback when
Claude runs out of credits, see [the agent handoff record](AGENT_HANDOFF.md).

| Person | Owns | Integration deliverable | Depends on |
| --- | --- | --- | --- |
| 1 — Organization kernel and Elastic | Rust/Axum/Tokio kernel, typed lifecycle models, mission/role/responsibility state, organization revisions, jobs/outbox, permissions, Elastic HTTP adapter and native retrieval | A validated organization change creates a real Elastic specialist and changes actual ownership/dispatch. Approved manifests are discoverable through native retrieval. | Shared contracts; readiness/results from Persons 3 and 4; contractor results from Person 2 |
| 2 — Fetch and demo UI | Isolated Python uAgents service and independently running specialist, discovery/recruitment protocol, Rust-facing JSON bridge, React mission/organization/evidence views | Live search → offer → acceptance → delegated result across the Rust/Python boundary. UI renders backend state and event receipts, including A/B organization separation. | Kernel event/snapshot and bridge contracts; actual provider receipts from all owners |
| 3 — Capability creation and verification | Public fixture portal, canonical oracle, TypeScript Actor scaffold, source generation, Rust Apify HTTP adapter, acceptance checks | A newly generated parser passes held-out checks; a manifest pins its exact approved build and source digest. | Capability request contract from Person 1; gateway authorization contract |
| 4 — CREAO, continuing work, and inheritance | Rust CREAO HTTP adapter, custom bridge skill, persistent monitor role, adoption/handoff integration | Newly created CREAO role handles two distinct events; A hands off and retires its temporary role; B discovers, locally checks, and reuses the package with its own role/state. | Kernel timers/assignments from Person 1; approved package from Person 3; discovery from Elastic |

Person 1 owns the integration trunk and shared contract changes. Each owner implements and smoke-tests their adapter, including failure responses. Person 4 leads the full A → B acceptance run; Person 2 leads the presentation and recording. These are ownership boundaries, not permission gates: pair on integration failures immediately.

## Contracts to freeze in the first two hours

Agree on small schema-validated JSON envelopes before writing provider-specific orchestration. Define canonical Rust structs and tagged enums with `serde`; model lifecycle states and allowed transitions explicitly, and validate semantic invariants before committing a transition. Generate or maintain matching TypeScript types and Python boundary models from the agreed schemas. These are GENESIS contracts, not claimed sponsor SDK methods. Compiler-checked types complement runtime validation and database constraints; they do not by themselves guarantee correct distributed behavior.

| Contract | Minimum fields and semantics | Producer → consumer |
| --- | --- | --- |
| Domain event | `schema_version`, `event_id`, `org_id`, `mission_id`, `correlation_id`, `causation_id`, timestamp, event type, payload; backend assigns a durable sequence | All adapters/kernel → kernel and UI |
| Organization change | Expected revision, observed requirement/evidence, role charter, responsibility transfers, authority limits, capability requirements, expected effect | Elastic proposal → kernel validation/provisioning |
| Provider binding | Internal role/assignment IDs, provider, provider resource ID, configuration digest, lifecycle/readiness state | Provider adapters → kernel |
| Contractor protocol | Typed `RecruitRequest`, `Offer`, `ContractAccepted`, `TaskAccepted`, `TaskProgress`, `TaskResult`, `ReleaseContract`; correlation, contract, task, attempt, deadline, schema version | Rust kernel ↔ Python uAgents service ↔ independent Fetch specialist |
| Capability request/result | Required behavior, input/output schema, allowed fixture URLs, acceptance-suite reference; return source digest, Actor ID, build ID/number, run/dataset IDs, test evidence | Kernel ↔ Apify/verification adapter |
| Published capability package | Intent, applicability, role/workflow template, exact executable version, I/O schemas, required local configuration/permissions, tests, evidence, provenance | Verifier/publication service → Elastic → Org B |
| Capability invocation | Organization, assignment/epoch, grant, logical operation ID, pinned capability version, inputs; validated output plus evidence | CREAO/internal roles → GENESIS gateway → Apify |
| UI state | Authoritative snapshot with revision/event cursor plus ordered events; reconnect can recover missed events | Kernel → React |

Keep role IDs, agent runtime/provider IDs, responsibility IDs, and assignment IDs separate. Every accepted responsibility has one accountable owner. A delegation can change the executor without changing accountability. Preserve raw provider responses separately from validated domain outputs.

No adapter writes organizational ownership directly. It returns receipts/results to the kernel. The Python Fetch service has no direct access to the kernel database or authority to activate roles: use an authenticated internal JSON boundary with request deadlines, correlation IDs, retry-safe operation IDs, and health/readiness checks. Restarting the bridge must not erase Rust-owned contracts or accepted work. No UI animation establishes success before a backend state transition does.

## Hours 0–4: live API gates

Run the four platform gates in parallel. Save sanitized requests, response shapes, resource IDs, and measured durations. Never put keys in Git, the UI, published manifests, or generated Actor source.

| Owner | Required smoke test | Pass evidence / important constraint |
| --- | --- | --- |
| 1 — Elastic | On an actual Serverless project, create an Agent Builder agent, create a native index-search tool and a skill, bind them using the documented schema, and invoke through converse. Retrieve a seeded capability document. | Real agent/tool/skill IDs, successful turn, and retrieval evidence. Distinguish Kibana management URL from Elasticsearch data URL. Verify API key privileges. Do not assume `converse/async` supplies durable background scheduling. |
| 2 — Fetch | Use the Python uAgents service to register an independently running specialist; find it through `POST https://agentverse.ai/v1/search/agents`; complete a typed message round trip and carry its request/result across the Rust-facing JSON bridge. | Search returns its address; live request/accept/result receipts correlate correctly across both languages. Measure registration/search visibility delay. A discovered address alone is not recruitment. |
| 3 — Apify | Through the documented REST API, create a source-file Actor, build it, run a pinned build on the public fixture, and read its dataset. Include an input schema. | Actor/build/run/dataset IDs and oracle-checked output. Confirm the backend forge token can create Actors; scoped tokens cannot. Measure cold build time and set explicit run limits. |
| 4 — CREAO | With the account `/v1` API, create an agent, poll the creation run until its agent ID exists, execute it, and inspect memory behavior. Install a generic custom skill with a supporting HTTP script and prove it can call the GENESIS gateway. | Real creation/run IDs and validated gateway receipt. Skill installation success does not establish sandbox readiness: execute the bridge. Confirm account quotas. Do not assume native schedule creation, team-agent management, or agent deletion exists in this API. |

At hour 4, report each gate as **passed**, **failed with reproducible error**, or **not yet tested**. Missing access or a failing bridge is a critical-path blocker, not a reason to label a mock as a live integration. Continue independent work on contracts, fixtures, kernel, and UI while resolving it. Explicitly record any changed demo scope.

CREAO account skills and secrets are account-wide; workspaces are not a secret-isolation boundary. Use public synthetic demo data and gateway-enforced, scoped grants. Keep the Apify forge token in the trusted backend. GENESIS timers invoke CREAO runs; no native schedule API is required.

## Milestones and integration order

| Elapsed time | Integrated milestone | Definition of done |
| --- | --- | --- |
| 0–4 h | Contracts and API proof | All four gates have evidence/status; response shapes and measured latency are recorded; Rust `serde` state/event schemas, fixture schemas, and Python bridge contract agreed. |
| 4–12 h | Runnable vertical skeleton | Rust kernel persists a mission, responsibility, job, and event; render a real snapshot; Fetch delegation crosses the isolated Python service; Actor produces oracle-checked data; CREAO bridge invokes the gateway. Temporary adapter stubs are explicitly labeled development-only. |
| 12–24 h | Org A acquires a new function | A requirement causes a justified mutation, real Elastic specialist creation, Fetch recruitment, runtime parser generation, independent verification, and activation of new ownership/capability bindings. Existing program work continues. |
| 24–34 h | Persistence and inheritance | CREAO handles a second event; responsibility handoff succeeds; temporary role retires; isolated Org B searches, locally checks, and executes the approved build with different mission inputs and no new source synthesis. |
| 34–42 h | Failure handling and evidence UI | Test partial provisioning, duplicate/late events, failed verification, incompatible adoption, and retirement. UI connects mission changes to organization revisions and actual provider receipts. |
| 42–48 h | Rehearsal and submission | Run withheld fixture variants; record total/provider durations, synthesis attempts, interventions, and observed costs; produce a local recording and honest fallback path. |

For a 36-hour cutoff, aim to finish the complete cycle by hour 30 and reserve the last six hours for verification and recording. Cut visual polish and optional repair before cutting continuing responsibility, inheritance, or real sponsor execution.

## Integration rules that prevent late failures

- **Provision before activation.** Persist a recoverable provisioning sequence. New roles remain non-executable until resources and checks are ready. Commit ownership changes atomically in the kernel database; Elastic is a searchable projection, not the transaction boundary.
- **Fence stale work.** Dispatch and results carry assignment epochs. A retired or superseded worker cannot change current commitments. Hand off unfinished work before retirement.
- **Make effects idempotent.** Expect retries and duplicate delivery. Use logical operation IDs and a durable outbox. Do not promise global exactly-once behavior across sponsor APIs. Simulated commitment writes should have a uniqueness constraint.
- **Publish only approved versions.** Pin Actor build and source digest. A successful build or provider run is not semantic verification. The canonical fixture oracle and acceptance conditions remain independent of generated code.
- **Make B actually independent.** Separate runtime/state stores and authority. B receives the published package, not A's conversation, secrets, or private connector IDs. Log the reused build digest, B's local tests, new role ID, and zero compatible-path source-synthesis calls.
- **Use one CREAO limiter.** Coordinate polling and management writes across roles; creation/invocation can be asynchronous. Preserve raw responses and validate domain results before changing mission state.
- **Measure the cold path early.** Actor builds and agent creation may exceed a three-minute pitch. A recording may demonstrate the completed live pipeline; a replay must be labeled replay, never fresh generation.

## Demo exit checklist

1. The requirement arrives after mission start; the generated role and parser are absent initially.
2. The new role has distinct accountable work, authority, and future event dispatch; it is more than a new prompt label.
3. An independent Fetch specialist is discovered and accepts bounded work.
4. A new Apify Actor runs and passes held-out checks before binding.
5. The new CREAO responsibility executes on two distinct events.
6. Handoff and retirement stop old dispatch without losing unfinished commitments.
7. Org B discovers the approved package and runs its pinned artifact against B's own input after local checks.
8. Mission outcomes, organization changes, and provider evidence agree in the UI.
9. At least one fixed-organization comparison and one no-inheritance comparison are recorded; report measured results without assuming the dynamic system wins.

Stretch only after the checklist: one bounded parser repair after layout drift. Defer payments, a public marketplace, arbitrary recursive spawning, production tenant isolation, and unrestricted self-modification of the kernel.
