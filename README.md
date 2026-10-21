# GENESIS

**A self-expanding operating system for autonomous organizations.**

GENESIS changes the organization a mission needs: it creates and recruits specialist agents, transfers responsibilities, develops missing executable capabilities, and lets independent organizations inherit verified capabilities.

The Rust capability implementation now creates source-bearing Actors, resumes pinned builds and runs, verifies results against an independent oracle, and checkpoints the forge across process restarts. A runnable TypeScript scaffold supplies the extraction runtime. The organization API now uses backend-owned proofs and durable execution for independent capability adoption in [PR #5](https://github.com/HenryWashuHe/genesis/pull/5). Kernel lifecycle/UI review fixes and authenticated provider smoke tests are still pending.

## Start here

1. [Technical specification](docs/TECH_SPEC.md) — architecture, data model, lifecycle, interfaces, and acceptance criteria.
2. [Team workstreams](docs/TEAM_WORKSTREAMS.md) — four parallel workstreams and integration milestones.
3. [Provider contracts](docs/PROVIDER_CONTRACTS.md) — documented APIs and the smoke tests that must pass first.
4. [Research report](GENESIS-organization-evolution-research.md) — competitive research, sponsor division, risks, and demo design.
5. [Capability forge integration](docs/FORGE_INTEGRATION.md) — runnable commands, source packaging, checkpoint recovery, and the kernel boundary.
6. [Organization capability API](docs/CAPABILITY_EXECUTION.md) — trusted publication, independent adoption, persisted runs, and recovery.
7. [Persistent CREAO specialists](docs/CREAO_RUNTIME.md) — asynchronous creation/execution, shared account limits, recovery and the pending kernel hook.
8. [Current implementation split](docs/NEXT_IMPLEMENTATION_SPLIT.md) — Codex capability work and Claude kernel/UI/Elastic assignments.

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

## MVP

An event organization loses its venue. GENESIS creates a relocation unit, recruits an external logistics specialist, generates and verifies a venue-availability Actor, and establishes a persistent CREAO venue monitor. A second organization discovers and adopts the resulting capability. The first organization retires its temporary relocation team after handing off outstanding work.

The fixture environment and reservations are simulated. Provider agent creation, Fetch messages, Actor builds and runs, and capability discovery must be real and visible in the evidence log.

## Working together

Consult the [handoff record](docs/AGENT_HANDOFF.md) before changing an active workstream. Use feature branches and pull requests; keep provider credentials and raw runtime logs out of Git. Commit and push working increments so teammates on other machines can integrate them.
