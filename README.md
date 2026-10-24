# GENESIS

![tag:innovationlab](https://img.shields.io/badge/innovationlab-3D8BD3)
![tag:hackathon](https://img.shields.io/badge/hackathon-5F43F1)

**The operating system that lets an AI organization change shape, and keep what it learns.**

Every agent framework ships a fixed org chart. GENESIS runs an organization as
durable state instead: roles, responsibilities, authority, budgets, contracts
and a constitution. When evidence arrives, a planner proposes the smallest
valid change and the kernel decides. Roles are created, specialists are
recruited, missing abilities are built and verified, work is handed off, and
temporary teams retire. Every revision is kept, and every state is audited.

What one organization builds, another can inherit. A verified capability is
published with its exact build, its acceptance suite and its provenance. A
second organization checks it against its own suite and installs it without
a single model call.

## Why it matters

- **Accountability survives change.** Every requirement has exactly one
  accountable owner with scopes, a budget and an epoch. A late result from a
  retired owner is rejected, not applied.
- **Abilities outlive teams.** The relocation team disbands; the venue
  capability stays, pinned and verified, for anyone the registry trusts.
- **The model proposes, the kernel decides.** Invalid proposals are rejected
  with exact error codes. The organization never acts on a guess.

## 60-second tour

1. A venue closes. The organization grows a Relocation lead that was not in
   the code, recruits an independent logistics specialist over Fetch.ai, and
   requests a capability it lacks.
2. GENESIS writes an Apify Actor for the unfamiliar portal, builds it, and
   verifies it against pages the model never saw.
3. A second disruption is handled by the existing CREAO monitor. No new role.
4. Org B, a separate organization, inherits the capability with zero
   synthesis calls.
5. The temporary role retires after handoff, and the audit strip shows every
   revision still satisfies the kernel invariants.

Sponsor integrations are real code with live gates; see the honesty table in
[docs/DEMO_RUNBOOK.md](docs/DEMO_RUNBOOK.md) for what is live, fixture or
pending at any moment. Fixture runs are labeled on screen and are never
presented as live.

## Start here

1. [Technical specification](docs/TECH_SPEC.md) — architecture, data model, lifecycle, interfaces, and acceptance criteria.
2. [Team workstreams](docs/TEAM_WORKSTREAMS.md) — four parallel workstreams and integration milestones.
3. [Provider contracts](docs/PROVIDER_CONTRACTS.md) — documented APIs and the smoke tests that must pass first.
4. [Research report](GENESIS-organization-evolution-research.md) — competitive research, sponsor division, risks, and demo design.
5. [Capability forge integration](docs/FORGE_INTEGRATION.md) — runnable commands, source packaging, checkpoint recovery, and the kernel boundary.
6. [Organization capability API](docs/CAPABILITY_EXECUTION.md) — trusted publication, independent adoption, durable runs, fresh invocation proofs, and recovery.
7. [Persistent CREAO specialists](docs/CREAO_RUNTIME.md) — asynchronous creation/execution, shared account limits, recovery and the pending kernel hook.
8. [Independent Fetch specialists](docs/FETCH_RUNTIME.md) — discovery, signed transport, durable recruitment, restart and release isolation.
9. [Current implementation split](docs/NEXT_IMPLEMENTATION_SPLIT.md) — Codex capability work and Claude kernel/UI/Elastic assignments.
10. [Capability source synthesis](docs/CAPABILITY_SYNTHESIS.md) — public requirement/examples to a durable generated parser and immutable forge candidate.
11. [Durable capability acquisition](docs/CAPABILITY_ACQUISITION.md) — one checkpoint from generation through build and verification, with one bounded semantic repair.
12. [Server-managed acquisition](docs/SERVER_ACQUISITION.md) — mission/assignment binding, persistent count quotas, resumable HTTP steps and trusted publication proofs.
13. [Two-organization inherited mission](docs/INHERITED_MISSION.md) — Rust scenario driver connecting specialist creation, acquisition, independent adoption and responsibility handoff.

## Run the capability checks

With Rust 1.94 or newer (including rustfmt and Clippy) and Node.js 24 or newer:

```sh
bash scripts/check-capability-core.sh
```

These checks use loopback provider simulations and the actual Apify SDK with local storage. They do not claim a live cloud build or AI source generation. See the integration guide for the durable CLI and the remaining live gates.

## Stack decision

| Layer | Planned technology |
|---|---|
| Organization kernel, API, scheduler, capability gateway | **Rust**, Axum, Tokio |
| HTTP provider adapters and typed contracts | Rust, reqwest, serde |
| Authoritative local state | SQLite; transactional mutations and a durable outbox |
| Mission and organization interface | TypeScript, React |
| Generated Apify Actors | TypeScript with a fixed build scaffold |
| Fetch agent transport | A small Python uAgents sidecar behind a typed local API |

Rust is the core implementation language. The Fetch sidecar keeps the official Python SDK isolated from the organization kernel. Elastic, Apify, and CREAO integrations use HTTP. The implemented crates and Actor scaffold include dependency lockfiles.

## Sponsor responsibilities

- **Fetch.ai:** discover and recruit independently running specialists.
- **Elastic Agent Builder on Serverless:** create internal specialists and skills; retrieve organization evidence and shared capabilities.
- **Apify:** create, build, and execute missing web capabilities.
- **CREAO:** create reusable agents for continuing operational responsibilities.
- **GENESIS:** own roles, authority, budgets, assignments, lifecycle, verification gates, and adoption decisions.

[Apify sponsor announcement notes](docs/APIFY_SPONSOR_NOTES.md) cover the announced credits, meaningful-use demo and separate AI Chat category.
[CREAO sponsor requirements](docs/CREAO_SPONSOR_NOTES.md) track core agent work,
agent ID/run-share-link submission evidence and the remaining live mission gap.
See the [PR merge/review queue](docs/PR_REVIEW_QUEUE.md) for what is on main and what remains held.

## MVP

An event organization loses its venue. GENESIS creates a relocation unit, recruits an external logistics specialist, generates and verifies a venue-availability Actor, and establishes a persistent CREAO venue monitor. A second organization discovers and adopts the resulting capability. The first organization retires its temporary relocation team after handing off outstanding work.

The fixture environment and reservations are simulated. Provider agent creation, Fetch messages, Actor builds and runs, and capability discovery must be real and visible in the evidence log.

## Working together

Consult the [handoff record](docs/AGENT_HANDOFF.md) before changing an active workstream. Use feature branches and pull requests; keep provider credentials and raw runtime logs out of Git. Commit and push working increments so teammates on other machines can integrate them.
