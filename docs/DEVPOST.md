# GENESIS: Devpost submission draft

**Networked recursive capability improvement with fixed model weights.**

Status: working draft, not ready to submit. Codex's previous implementation Goal and recurring work remain
paused; colleagues' existing scoped claims continue. Confirm every accomplishment against [implementation status](IMPLEMENTATION_STATUS.md),
and replace each `TODO-LIVE:` with actual evidence before claiming a sponsor
integration. The [invention-lab demonstration](INVENTION_LAB_DEMO.md) is
proposed; its outcomes must not be described as completed work.

## Inspiration

An organization can retain more than the answer to its last task. It can
retain a better way to investigate, build and test the next one. What if
independent AI organizations could exchange those improvements, while each
kept control of its own mission and resources?

GENESIS explores that possibility without changing the underlying model
weights. Its ambition is a network where organizations create, verify, share
and selectively inherit executable capabilities, including the methods used
to develop future capabilities. One organization's experience could improve
how another organization produces its next improvement.

## What it does today

GENESIS implements foundations for that network. A Rust kernel governs
organization state: roles, responsibilities, assignments, authority, budgets
and contracts. It validates proposed changes, records committed revisions,
and fences work whose authority is no longer current. A shared registry
carries versioned capabilities; receiving organizations run their own
acceptance checks before adoption.

The current executable acquisition path generates TypeScript web-extraction
candidates within an Apify Actor scaffold. It pins artifacts, verifies output
against independently configured acceptance data, and permits a bounded
semantic repair during acquisition. It is not yet a general executable
development-workflow or physics runtime.

A two-organization integration exercises A creating, verifying and publishing
a capability, then B discovering, checking and installing it without B
synthesizing another copy. That integration uses explicitly labeled loopback
model/provider responses. Separate local signed Fetch-process evidence
exercises specialist selection and retirement with other providers simulated.
These results establish bounded mechanisms, not live cloud completion or
recursive improvement.

## Proposed demonstration: autonomous invention labs

Lab A must design a machine to move cargo across a ravine in a simulated
physics world. It develops an executable method for searching designs and
choosing experiments, then publishes a verified version with documented
scope and limitations.

Independent Lab B investigates a robot that must handle uneven terrain and
gusts. It checks whether A's method fits. If interacting conditions expose a
weakness, B can develop a new method that retains counterexamples and tests
designs across relevant scenarios. This is a candidate direction for a
system-generated change, not a hardcoded upgrade or a promised result.

After that candidate is frozen, a judge reveals a new perturbation for A.
A independently accepts or declines the method and attempts a new design.
A control uses the previous method from the same initial state under the
same declared budget. The evaluator and hidden acceptance criteria remain
outside both organizations' editing authority.

The visible result should be machines failing, experiments exposing why, and
new designs succeeding if the system finds them. The critical evidence is
the A → B → A transfer of an improved **development method**, followed by an
independent comparison. Passing a finished design between labs is ordinary
reuse. A method that fails to help must be reported as such.

The scene is a research environment for testing the architecture. It would
not establish real-world engineering safety or general scientific ability.
Historical venue fixtures remain infrastructure tests; event organizing is
no longer the product direction or a required demonstration.

## How the foundations are built

- **Rust, Axum, Tokio and SQLite:** organizational authority, durable state,
  provisioning and execution records, version identity, and recovery states.
- **Planner:** structured proposals are checked by the kernel. Provider
  integrations support model-backed planning; a valid proposal does not
  prove the plan is optimal or scientifically correct.
- **Capability acquisition and verification:** backend-controlled source
  generation, pinned Apify artifacts, independent acceptance data, bounded
  attempts and immutable evidence.
- **Cross-organization adoption:** shared discovery and organization-local
  acceptance. Publishing a replacement does not automatically upgrade
  another organization's installation.
- **Provider adapters:** Elastic Agent Builder/evidence retrieval, Fetch
  specialist messaging, Apify Actor operations and CREAO operational roles.
  Each integration's live evidence is tracked separately.
- **React console:** organization state, capability progress, evidence,
  provenance and audit views. A visual transition is not itself proof of an
  external effect or a successful mission.

## Challenges and what we learned

Retaining tools is accumulation. Executing them is reuse. Recursive
capability improvement requires something stronger: an inherited change to
the development process must improve later development on independently
evaluated tasks. More tools, more agents or a shorter trace cannot establish
that by themselves.

The other challenge is preserving authority and evidence across independent
organizations. Provider completion is distinct from a correct result;
publisher verification is distinct from receiving-organization acceptance.
Ambiguous external effects can require reconciliation instead of another
submission. These boundaries matter even when a demonstration uses fixtures.

## Current accomplishments and limits

- Implemented governed organizational mutations, capability acquisition and
  versioned independent adoption with focused local tests.
- Exercised the two-organization creation/publication/adoption path with
  loopback providers and no B synthesis.
- Composed automatic recovery intake with actual signed local Fetch
  processes; the remaining providers in that test are fixtures.
- Recorded source-specific evidence and remaining gates in
  [implementation status](IMPLEMENTATION_STATUS.md).

The proposed invention labs, executable development-method inheritance and
measured recursive benefit are not completed accomplishments. Deployed
capability repair work remains paused and unmerged. The merged ACP mission
adapter has outstanding review regressions. No complete public ASI:One or
live sponsor mission is established by the evidence above.

## What's next

The long-term product is infrastructure for autonomous research and
engineering organizations to exchange verified improvements in how they
develop capabilities. Organizational evolution changes accountable
responsibilities and specialist participation when warranted. Capability
evolution changes executable methods and tools while preserving local
authority, exact versions and independent evaluation.

The proposed lab requires a physics executor, domain-specific evaluation and
executable development-method contracts beyond the present extraction path.
The [recursive roadmap](RECURSIVE_CAPABILITY_IMPROVEMENT.md) separates
accumulation, reuse/composition and measured recursive improvement. It calls
for fixed-model comparisons, total cost accounting, unsuccessful runs and
appropriate non-reuse. Broader transfer remains a research question.

## Built with

Rust, Axum, Tokio, SQLite, TypeScript, React, Vite and Python; integrations for
uAgents/Fetch.ai, Elastic Agent Builder on Elasticsearch Serverless, Apify,
CREAO and model providers. Integration code is not a claim of authenticated
execution; see the evidence below.

## Development disclosure

Before submission, record the actual event window and identify pre-existing
work versus work completed during it from repository history. Attach exact
source revisions and run evidence for the submitted demonstration. Do not
infer that a provider run occurred from the date its adapter was committed.
Do not publish the current draft's proposed outcomes as accomplishments.

## Sponsor evidence

The proposed roles below must be implemented and demonstrated before they
become submission claims. They describe GENESIS responsibilities, not new
sponsor API guarantees. Current submission requirements are recorded in the
linked sponsor notes; account setup or a logo alone does not meet them.

### Fetch.ai

![tag:innovationlab](https://img.shields.io/badge/innovationlab-3D8BD3)
![tag:hackathon](https://img.shields.io/badge/hackathon-5F43F1)

Proposed role: discover and recruit independent lab specialists, with an ACP
agent exposing the complete primary workflow through ASI:One. Local signed
transport tests do not prove public discovery or ASI:One completion.

| Agent | Address | Proposed responsibility |
| --- | --- | --- |
| GENESIS Organization | `TODO-LIVE: agent1q…` | ACP entry point for the complete primary workflow |
| GENESIS Research Specialist | `TODO-LIVE: agent1q…` | Independently recruited experimental or mechanism specialist |

- Agentverse registration/profile: `TODO-LIVE: https://agentverse.ai/agents/details/…`
- ASI:One shared conversation completing the primary workflow: `TODO-LIVE: https://asi1.ai/…`
- Public GitHub submission repository: `TODO-LIVE: public repository URL`.
  Visibility changes remain a separate owner decision.
- Setup and evidence requirements: [Fetch sponsor notes](FETCH_SPONSOR_NOTES.md)
  and [bridge README](../services/fetch-bridge/README.md).

### CREAO

Proposed core work: a persistent experiment analyst executes the lab's
analysis and follow-up responsibility across iterations. This workflow is
not implemented by the current venue-specific assessment path.

- Agent built on agent.creao.ai that actually performs core work:
  `TODO-LIVE: agent ID`
- Genuine run share link: `TODO-LIVE: https://…` (the CREAO Share link, not a run ID)
- Core output and its effect on the lab's next action:
  `TODO-LIVE: evidence reference`
- Proposed track: Best AI-First Disruptor; select the track when submitting.
- Requirements: [CREAO sponsor notes](CREAO_SPONSOR_NOTES.md).

### Apify

Proposed role: build and execute generated software Actors for a compatible
bounded experiment worker or development tool used by the labs. This requires
a new validated scaffold/executor; the current Actor path supports extraction
and does not implement a physics lab. Check the selected platform contract,
independently evaluate output and record its contribution before claiming
this role is implemented.

- Actor: `TODO-LIVE: actor ID`
- Build: `TODO-LIVE: build ID, build number`
- Verification runs: `TODO-LIVE: run IDs, dataset IDs and actual acceptance results`
- Evidence of the Actor output affecting a design decision:
  `TODO-LIVE: input/output/decision references`
- Independent adoption run, if demonstrated:
  `TODO-LIVE: organization and run ID for the same pinned build`
- Requirements and separate AI Chat category: [Apify sponsor notes](APIFY_SPONSOR_NOTES.md).

### Elastic Agent Builder on Serverless

Proposed role: specialist agents retrieve scoped experimental evidence and
candidate methods, helping the labs select, develop and evaluate capabilities.
Show actual use of Agent Builder and retrieval, not only an indexed record.

- Project: `TODO-LIVE: project name/region` (no URLs containing keys)
- Evidence/capability index: `TODO-LIVE: index name`
- Specialist agent ID: `TODO-LIVE: agent ID`
- Planner agent ID, if the planner ran on Agent Builder: `TODO-LIVE: agent ID`
- Retrieval/agent execution and resulting decision: `TODO-LIVE: evidence references`
- Smoke report: `TODO-LIVE: data/live-gates/gates-<run>.md`

Authenticated cloud runs establish provider execution. They do not by
themselves establish recursive improvement; that requires the independently
evaluated method-transfer experiment.
