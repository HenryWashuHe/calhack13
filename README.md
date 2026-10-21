# GENESIS

**A self-expanding operating system for autonomous organizations.**

GENESIS changes the organization a mission needs: it creates and recruits specialist agents, transfers responsibilities, develops missing executable capabilities, and lets independent organizations inherit verified capabilities.

This repository currently contains the research, technical specification, provider contracts, and team workstreams. Application implementation has not started. Provider APIs have been checked against official documentation; authenticated integration checks are still pending.

## Start here

1. [Technical specification](docs/TECH_SPEC.md) — architecture, data model, lifecycle, interfaces, and acceptance criteria.
2. [Team workstreams](docs/TEAM_WORKSTREAMS.md) — four parallel workstreams and integration milestones.
3. [Provider contracts](docs/PROVIDER_CONTRACTS.md) — documented APIs and the smoke tests that must pass first.
4. [Research report](GENESIS-organization-evolution-research.md) — competitive research, sponsor division, risks, and demo design.

## Stack decision

| Layer | Planned technology |
|---|---|
| Organization kernel, API, scheduler, capability gateway | **Rust**, Axum, Tokio |
| HTTP provider adapters and typed contracts | Rust, reqwest, serde |
| Authoritative local state | SQLite; transactional mutations and a durable outbox |
| Mission and organization interface | TypeScript, React |
| Generated Apify Actors | TypeScript with a fixed build scaffold |
| Fetch agent transport | A small Python uAgents sidecar behind a typed local API |

Rust is the core implementation language. The Fetch sidecar keeps the official Python SDK isolated from the organization kernel. Elastic, Apify, and CREAO integrations use HTTP. Exact dependency versions will be pinned when implementation begins.

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

Choose a workstream in the team document before implementation. Define shared Rust types and JSON contracts first, then implement adapters against those contracts. Use feature branches and pull requests; keep provider credentials and raw runtime logs out of Git. There are no application start commands or passing test claims yet.
