# GENESIS: Devpost submission text

Paste the sections below into Devpost. Fill every `TODO-LIVE:` line with the
real identifier from a live run before submitting. Do not submit a
placeholder.

## Inspiration

Every agent framework we tried ships a fixed org chart. You pick the agents,
wire the tools, and hope the task fits. Real organizations are different.
They hire when the work changes, build tools they lack, hand work off, and
dissolve teams while keeping what they learned. We wanted an operating system
that lets an AI organization do that, with the safeguards a real company
needs: authority, budgets, verification, and an audit trail.

## What it does

GENESIS runs an organization as durable state: roles, responsibilities,
scopes, budgets, assignments and contracts, governed by a constitution. When
evidence arrives, a planner proposes the smallest valid organizational change.
The kernel validates it, provisions what it needs, and commits it as a new
revision with an event log.

The demo is an event company whose venue closes. GENESIS creates a relocation
role, recruits an independent logistics specialist, writes and verifies a web
capability it did not have, hands continuing monitoring to a persistent agent,
and retires the temporary role. A second, independent organization then
discovers that capability, checks it against its own acceptance suite, and
installs it with zero model calls. A read-only audit checks every revision
ever stored against the kernel invariants.

## How we built it

- **Rust kernel** (Axum, Tokio, SQLite): validation, a provisioning saga with
  durable outbox, epoch fencing, append-only history, SSE event stream, and a
  shared capability registry.
- **Planner**: a domain-neutral system prompt describes only the mutation
  vocabulary and decision rules. Role names and charters come from the
  evidence. The model proposes; the kernel decides. Runs on Anthropic,
  OpenAI-compatible endpoints, or an Elastic Agent Builder agent.
- **Elastic Agent Builder on Serverless**: every organization projects its
  missions, observations, requirements and roles into its own evidence index.
  Internal specialist roles become Agent Builder agents whose only tool is an
  index search scoped to that organization. The planner itself can run as an
  Agent Builder agent that consults the evidence before proposing.
- **Fetch.ai**: specialists are discovered on Agentverse and recruited as
  independent uAgents under signed, durable contracts. A separate ACP agent
  makes the primary workflow usable from an ASI:One conversation: report the
  disruption, watch the organization evolve, read the result.
- **Apify**: when the organization lacks an executable capability, GENESIS
  synthesizes Actor source from the public requirement and example pages,
  creates the Actor, builds it, pins the build, runs it, and verifies the
  output against a held-out oracle the model never sees. One bounded semantic
  repair is allowed. The verified pin is what the registry publishes.
- **CREAO**: continuing operational work lives in a persistent CREAO agent.
  After the temporary relocation role hands off, the same agent assesses two
  later venue updates and its verified assessments change the mission.
- **Console** (React): org tree, evidence log, capability chips, Attention
  panel, and provenance views: why each role exists, how the organization
  evolved, replay by revision, and the kernel audit strip.

## Challenges

Keeping provider success separate from domain truth. An Actor run finishing is
not a correct extraction, and a CREAO run completing is not a mission
outcome. Every provider result enters as evidence and is verified before it
changes the organization. Recovery was the other half: lost responses,
restarts mid-provisioning, late results from retired roles. The kernel refuses
to guess, records `needs_reconciliation`, and surfaces it in the Attention
panel instead of repeating an effect.

## Accomplishments

- A role appears, does its job, hands off, and retires, and the history
  proves it.
- An organization grows a verified web capability it did not have.
- A second organization inherits it with zero synthesis calls.
- Two different disruptions produce two different organizational answers.
- 600 plus tests across the workspace, a crash test that restarts each
  organization at fixed points of the capability path, and a full-history
  invariant audit.

## What we learned

Organizational evolution is only real if ownership, authority, future
dispatch and handoff change. Otherwise it is orchestration with extra steps.
And the model is the least reliable component, so the kernel must be the
authority.

## What's next

A network of organizations. Each one changes shape as its mission changes,
and each one can inherit verified abilities from the others with provenance
attached. Capability packages carry intent, limits, contracts, acceptance
suites and known failures. Organizations recruit services from each other,
form temporary joint teams, and keep expertise when those teams disband. The
event agency is day one.

## Built with

Rust, Axum, Tokio, SQLite, TypeScript, React, Vite, Python, uAgents,
Elastic Agent Builder, Elasticsearch Serverless, Apify, CREAO, Anthropic
Claude.

## Disclosure

The repository was created on October 20, 2026 PT with research, the
technical specification and early kernel code. The ASI:One path, the live
sponsor runs, the provenance console fixes, the pitch and the end-to-end runs
were done during Cal Hacks 13.0. The commit log in the repository shows when
each piece landed.

## Sponsor evidence

### Fetch.ai

![tag:innovationlab](https://img.shields.io/badge/innovationlab-3D8BD3)
![tag:hackathon](https://img.shields.io/badge/hackathon-5F43F1)

| Agent | Address | Role |
| --- | --- | --- |
| GENESIS Organization | `TODO-LIVE: agent1q…` | ACP agent, primary workflow in ASI:One |
| GENESIS Logistics Specialist | `TODO-LIVE: agent1q…` | independent recruited specialist |

- Agentverse profile: `TODO-LIVE: https://agentverse.ai/agents/details/…`
- ASI:One shared conversation: `TODO-LIVE: https://asi1.ai/…`
- Run instructions: `services/fetch-bridge/README.md`, `docs/FETCH_SPONSOR_NOTES.md`

### CREAO

- Agent built on agent.creao.ai: `TODO-LIVE: agent ID`
- Run share link: `TODO-LIVE: https://…` (the Share link from CREAO, not a run ID)
- Core work: persistent venue monitor; two verified assessments after handoff
- Track: Best AI-First Disruptor

### Apify

- Actor: `TODO-LIVE: actor ID`
- Build: `TODO-LIVE: build ID, build number`
- Verification runs: `TODO-LIVE: run IDs, dataset IDs`, oracle verdict `TODO-LIVE: n/n`
- Org B adoption run of the same pinned build: `TODO-LIVE: run ID`

### Elastic Agent Builder on Serverless

- Project: `TODO-LIVE: project name/region` (no URLs with keys)
- Evidence index: `TODO-LIVE: genesis-evidence-…`
- Specialist agent ID: `TODO-LIVE: …`
- Planner agent ID (if the planner ran on Agent Builder): `TODO-LIVE: …`
- Smoke report: `TODO-LIVE: data/live-gates/gates-<run>.md`
