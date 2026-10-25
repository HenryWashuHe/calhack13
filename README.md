# GENESIS

Active development: [calhack13](https://github.com/HenryWashuHe/calhack13).
Read [coordination issue #1](https://github.com/HenryWashuHe/calhack13/issues/1) and the
[repository migration map](docs/REPOSITORY_MIGRATION.md) before taking work.

![tag:innovationlab](https://img.shields.io/badge/innovationlab-3D8BD3)
![tag:hackathon](https://img.shields.io/badge/hackathon-5F43F1)

**Networked recursive capability improvement with fixed model weights.**

Project timeline: **Friday, October 23 → Sunday, October 25, 2026 (Pacific)**.
See the [Friday-start timeline](docs/PROJECT_TIMELINE.md) and
[documentation guide](docs/README.md). Status reviewed against main `83e79c6a`.

GENESIS is building infrastructure through which independent AI organizations
create, verify, share, and improve executable capabilities—including the methods
they use to develop future capabilities. What evolves is the system around the
model: tools, workflows, memory, specialist roles and development procedures.
The foundation model's weights stay fixed.

Organizations are durable, governed state: responsibilities, roles, authority,
budgets and contracts. A planner can propose a structural change when a mission
needs new ownership; the Rust kernel validates it. Capabilities retain exact
versions, provenance and tested scope. Receiving organizations decide locally
whether to adopt, adapt, or decline them.

**Implemented foundations are not a demonstrated recursive-improvement system.**
The Rust kernel governs organizational changes and exact-version capabilities.
Merged development-method support includes source generation and revision,
registry exchange, receiver-controlled adoption, pinned invocation, experimental
execution/scoring, and retained cost accounting. Actual generated-method records
and a completed job-shop controlled rehearsal are published; their scopes differ.
An accepted independent A → B → A return and general recursive benefit remain
unproved. See [implementation status](docs/IMPLEMENTATION_STATUS.md).

## The proposed demonstration: invention labs

**One organization invents a machine. Another improves the way it invents.
The first inherits that method and tackles a new challenge.**

Lab A develops a cargo-crossing machine in a physics simulation and an executable
experiment method. Lab B evaluates that method for a different design problem,
improves the procedure, and publishes a new version. After the method is frozen,
A receives a fresh challenge and independently checks whether the inherited
procedure helps it develop a better solution under the same model and budget.
Moving machines make the result visible; method lineage and independent
comparisons establish what happened.

The complete demonstration remains **a research target, not a promised positive
result**. Simulator, executor/scorer, method registry, adoption and invocation
implementations are merged. Retained runs include failures and rejected returns;
independent organizational operation, runtime assurance and G0–G4 remain open.
The [demo design](docs/INVENTION_LAB_DEMO.md) specifies the missing pieces and falsifiable gates. The former event-organizer demo is
superseded; its runtime code remains a historical integration fixture.

## How the capability network develops

1. **Discover:** An agent encounters a requirement and searches for an existing capability that fits it.
2. **Organize:** If the capability is missing, the organization assigns responsibility and can create or recruit the specialists needed to develop it.
3. **Build and verify:** Specialists produce an executable candidate. Independent checks establish its tested behavior, artifact identity, permissions, and limitations.
4. **Publish and inherit:** The verified version enters a shared registry. Another organization discovers it and runs its own acceptance checks before adoption.
5. **Repair and build further:** Failures should lead to bounded, evidence-backed repairs. Useful capabilities should also become dependencies of new capabilities, including better builders and repair workflows.

Organizations are the authority boundary: each owns its missions, roles, budgets, grants, and adoption decisions. The shared registry connects independently operated organizations. Receiving a published capability does not automatically authorize its execution.

GENESIS supports two connected forms of evolution: **organizational evolution**, changing who is responsible and which specialists participate; and **capability evolution**, changing what those specialists can execute and share.

### What “recursive” means here

A growing capability library provides accumulation. Reusing or composing its contents can improve a task. The recursive step is an improvement that makes the system **better at producing its next improvement**, demonstrated on independent evaluation.

For example, one organization publishes an extractor; another develops a regression-test generator; a third uses both to improve a tool-building workflow. If the first organization can then adopt that workflow and build better tools on new tasks, the loop has closed. The trusted acceptance criteria remain independent of the component being improved.

This is the intended architecture. **The current implementation establishes important foundations; it does not yet demonstrate a general recursive self-improvement loop.** No model-weight training is required by this design, and there is no claim of autonomous model-weight improvement.

#### Universal infrastructure, selective transfer

A universal runtime does not require universal tools. An equation solver may help a later equation or a function plot and contribute nothing to drawing a cartoon cat. A capability can become useful many missions later; consecutive tasks need not share it. These are execution and evaluation tasks, not model-training steps.

GENESIS should retain useful experience, retrieve candidates, check applicability, and adopt only under the receiving organization's policy and independent acceptance evidence. **Finding no suitable capability is a normal outcome.** Reuse, composition, adaptation, fresh creation, and continuing with existing tools are all legitimate choices. A failed or unavailable search is an unknown outcome, not evidence that no suitable capability exists.

Tools, domain workflows, and methods for building or repairing tools have different scopes. Workflow structure may transfer while its evaluator does not: checking an equation and judging a drawing require different evidence. More stored capabilities do not guarantee higher quality, and even a rejected irrelevant tool can add retrieval cost. Tests reduce uncertainty; they cannot guarantee correct selection or eliminate negative transfer.

The research goal is measurable improvement on a declared mix of tasks, with failures and costs reported—not improvement after every task. See the [recursive capability improvement goal](docs/RECURSIVE_CAPABILITY_IMPROVEMENT.md) for implementation milestones, selective-transfer cases, and controlled evaluation.

## What the current implementation supports

| Mechanism | Current support and boundary |
| --- | --- |
| Organizational evolution | The Rust kernel supports role creation/retirement, specialist recruitment, responsibility assignment/delegation, and capability attachment. Proposals pass authority and lifecycle checks before committing. |
| Executable capability creation | The acquisition workflow generates TypeScript extraction candidates, packages source-bearing Apify Actors, pins builds/runs, and verifies outputs against an independently configured suite. This task-capability path is specialized to Apify Actors; development methods have a separate executable package/runtime contract. |
| Bounded repair | Acquisition can perform one planned semantic repair after a fully collected rejection, preserving lineage and rerunning verification. This is not yet a general repair service for deployed capabilities across the network. |
| Cross-organization inheritance | Immutable versioned manifests, shared discovery, adopt-before-acquire, and organization-local acceptance are implemented. A two-organization integration exercises A creating/publishing a capability and B adopting it without B synthesizing another copy. |
| Persistent specialist work | Fetch supplies independent signed specialist exchanges; CREAO runs app-backed role contracts; Elastic supports organizational evidence, retrieval, and Agent Builder integrations. End-to-end live sponsor evidence remains a separate gate. |
| Recovery and control | SQLite journals, exact artifact/request identities, authority checks, bounded attempts, and reconciliation states protect the tested workflows. Ambiguous external effects can require operator attention; recovery does not mean every operation can safely be replayed. |
| Development-method foundations | Exact source packages, generated revisions, registry publication/search/fetch, receiver-owned decisions, responsibility/grant binding, invocation records, optional rescoring/re-execution, and stage/chain cost accounting are merged. See the [method contract](docs/invention-lab/adoption.md). Retained evidence and local policy checks do not establish independently authenticated execution or complete spending history. |
| Recursive composition and network healing | Accepted independent method return and measured network benefit remain open. Shared repair advisories, general dependency resolution and controlled deployed-capability upgrades remain future work. |

A manifest already carries schemas, permissions, provenance, verification evidence, and optional role/workflow descriptions. The separate development-method packages supply executable source and pinned runtime/evaluator references; manifest descriptions alone are not an executor or a general dependency resolver. The network currently uses a shared registry service; it is not a decentralized peer-to-peer capability protocol.

Verification establishes behavior within the tested scope. It does not prove arbitrary generated code is safe, correct on every future input, or suitable for another organization's mission without that organization's checks.

## What can be run today

**Existing reproducible mechanism:** A requests a missing web capability; its
workers acquire, verify, publish, and locally accept it. Independent B discovers
that version, runs its own checks, and installs it without synthesizing another
copy. The default rehearsal seeds requests and uses explicitly labeled loopback
model/provider responses. Zero B synthesis is not a claim of zero model calls
through every organization or planner path.

This fixture demonstrates capability inheritance. It does not execute the
proposed invention-lab demo or establish measured recursive improvement.

The forge also provides offline preparation/status/export and one bounded model
request for method source; see the [method-generation CLI contract](docs/invention-lab/contracts/generation.md).
Its exact request survives restart, uncertain submissions never replay, and
source validity is separate from provider accounting. [Retained live generation](docs/invention-lab/evidence/live-method-20261024-generated/README.md)
records an actual model-produced package, following two failed attempts.
Provider-default sampling and separate generation costs are explicit limitations until the comparison settings and total budget are enforced.
An [optional admission journal](docs/invention-lab/contracts/generation-admission.md#cli)
reserves declared requests and requested maximum output before the existing
generation step. Failed or uncertain attempts retain their reservations; legacy
steps without the option remain ungated. These cooperative bounds do not measure
provider billing or enforce input-token/wall limits.

The [admission-aware cost assessment](docs/invention-lab/contracts/admitted-costs.md#cli)
adds `--admission` to the existing `method-cost-assess` command. It checks the
entire original roster and later observations, then joins generation and optional
trial costs while retaining unknown usage and overruns. It makes no writes or
provider calls. The [stage-allocation CLI](docs/invention-lab/contracts/stage-allocation.md#cli-and-retained-files)
can also retain an allocation from unused Prepared attempts, followed by one
exact trial binding. Optional `--allocation` checks that retained context during
generation and cost assessment. These are cooperating local-file mechanisms;
[whole-chain allocations](docs/invention-lab/contracts/chain-allocation.md) and
[operator invocation records](docs/invention-lab/contracts/method-runs.md) are
implemented, while authenticated chronology, exhaustive spending and independent
execution assurance remain open.

### Run the local inheritance rehearsal

With Rust 1.94 or newer:

```sh
cargo run -p genesis-server --example rehearse_autonomy
```

The example serves the registry and organizations on ports 7100–7102, runs A then B, and stays open for the console until Ctrl-C. Each launch uses fresh temporary databases. Provider responses are fixtures; this command does not establish live AI generation, cloud Actor execution, or restart recovery of those temporary organizations. See [development setup](docs/DEV.md) for the console.

### Run the capability checks

With Rust 1.94 or newer, rustfmt, Clippy, and Node.js 24 or newer:

```sh
bash scripts/check-capability-core.sh
```

These checks use loopback provider simulations and the actual Apify SDK with local storage. Authenticated cloud runs, live model quality, and sponsor submission evidence are tracked separately in [live gates](docs/LIVE_GATES.md).

## Published controlled rehearsal

The [October 25 job-shop report](experiments/jobshop-controlled/results/2026-10-25/README.md)
retains one preregistered, 50-request Sonnet 5.5 run and its local replay.
Feedback iteration scored **231/300**, versus **216/300** for equal-call
independent generation: a five-percentage-point difference on five paired seeds.
Random search beat feedback on four of five panels. This narrow result does not
establish cross-organization transfer, an accepted A → B → A loop, or G4.
The operator-calculated API cost was **$0.515122**, not a final provider invoice.

## The most valuable next improvements

1. **Strengthen execution evidence.** Validate the merged executor/scorer on the supported Linux environment, retain complete source-bound records and generation costs, and distinguish operator-run evidence from independent assurance. Preserve handwritten controls and negative outcomes.
2. **Close an independent A → B → A method loop.** Let B check and use A's exact method, develop its own executable revision, then let A independently accept and run that frozen revision on a fresh task. Preserve each organization's authority and valid non-reuse outcomes.
3. **Measure selective and recursive benefit.** Compare the same base system without cross-task accumulation, with task capabilities, and with task capabilities plus inherited development workflows on held-out tasks. Freeze the model and trusted evaluation, count creation/verification/retrieval costs as well as execution, report regressions by task family, and gate promotion on criteria fixed before evaluation. Generated tests can supplement the evaluation; they cannot replace its trusted acceptance criteria.

These are proposed implementation priorities, not completed features. Live sponsor validation proceeds alongside them; neither a passing fixture nor a larger capability count alone establishes recursive improvement.

The implementation sequence remains subject to the agreed development-method and evaluator contracts; the new demonstration need not use web extraction.

The [documented goal and acceptance criteria](docs/RECURSIVE_CAPABILITY_IMPROVEMENT.md#goal-and-completion-evidence) turn these priorities into a reviewable implementation target. Current assignments and resumption boundaries are recorded in the [handoff](docs/AGENT_HANDOFF.md#current-ownership-and-next-work).

## Sponsor responsibilities

| Platform | Contribution to the system |
| --- | --- |
| **Fetch.ai** | Discovery, communication, and recruitment of independently running specialists. |
| **Elastic Agent Builder on Serverless** | Organizational intelligence, specialist agents and skills, evidence retrieval, and searchable shared capabilities. |
| **Apify** | Creation, build, and execution of missing web capabilities as software Actors. |
| **CREAO** | Published-app API Trigger runs for GENESIS-owned responsibilities; exact bootstrap role contracts accompany jobs. Live gateway readiness remains a separate check. |
| **GENESIS** | Organization structure, authority, budgets, lifecycle, capability identity, verification, and adoption decisions. |

[Apify sponsor notes](docs/APIFY_SPONSOR_NOTES.md) explain meaningful-use evidence and the separate AI Chat category. [CREAO sponsor requirements](docs/CREAO_SPONSOR_NOTES.md) track actual core agent work, agent ID, and a genuine run share link. [Fetch sponsor requirements](docs/FETCH_SPONSOR_NOTES.md) track ACP, Agentverse discovery, and actual ASI:One use. Local signed-process results, loopback tests, and cloud-provider evidence must remain distinguishable.

## Stack

| Layer | Technology |
| --- | --- |
| Organization kernel, API, scheduler, capability gateway | Rust, Axum, Tokio |
| Provider adapters and typed contracts | Rust, reqwest, serde |
| Authoritative local state | SQLite, transactional mutations, durable journals/outbox |
| Mission and organization interface | TypeScript, React; standalone HTML Stage views |
| Experimental development methods | Python source packages, Rust executor/scorer, separate evaluator crates |
| Generated Apify Actors | TypeScript with a fixed build scaffold |
| Fetch agent transport | Python uAgents sidecar behind a typed local API |

The Rust kernel remains the authority boundary. Elastic, Apify, and CREAO integrations use HTTP; the Fetch sidecar isolates the official Python SDK from the kernel. This framing does not require a language or runtime rewrite.

## Start here

- [Documentation guide](docs/README.md), [project timeline](docs/PROJECT_TIMELINE.md), and [Stage demo](apps/stage/README.md): navigation, Friday-to-Sunday milestones, and labeled presentation modes.
- [Implementation status](docs/IMPLEMENTATION_STATUS.md) and [proposed invention-lab demo](docs/INVENTION_LAB_DEMO.md): what exists, what is unmerged, and what remains a research target.
- [Technical specification](docs/TECH_SPEC.md) and [research report](GENESIS-organization-evolution-research.md): architecture, original research, and design rationale. This README states the current product framing; older milestone/status prose in detailed guides should be read against the current handoff.
- [Capability execution](docs/CAPABILITY_EXECUTION.md), [adopt-before-acquire](docs/CAPABILITY_GAP_ADOPTION.md), and [automatic finalization](docs/CAPABILITY_GAP_FINALIZER.md): publication, independent acceptance, and inheritance.
- [Source synthesis](docs/CAPABILITY_SYNTHESIS.md), [durable acquisition](docs/CAPABILITY_ACQUISITION.md), [server-managed acquisition](docs/SERVER_ACQUISITION.md), and [forge integration](docs/FORGE_INTEGRATION.md): executable creation and bounded repair.
- [Inherited mission](docs/INHERITED_MISSION.md) and [Fetch venue recovery](docs/VENUE_RECOVERY.md): concrete cross-organization and specialist workflows, with their evidence limits.
- [Fetch runtime](docs/FETCH_RUNTIME.md), [CREAO runtime](docs/CREAO_RUNTIME.md), and [provider contracts](docs/PROVIDER_CONTRACTS.md): integration boundaries and setup.
- [Live gates](docs/LIVE_GATES.md), [handoff](docs/AGENT_HANDOFF.md), and [review queue](docs/PR_REVIEW_QUEUE.md): validation evidence, active ownership, and outstanding work.
- [Team workstreams](docs/TEAM_WORKSTREAMS.md) and [implementation split](docs/NEXT_IMPLEMENTATION_SPLIT.md): coordination history; consult the current handoff before taking ownership.

## Working together

The [ongoing goal](docs/ONGOING_GOAL.md) defines completion gates, the milestone ledger, and the recurring check/assign/implement/review cycle. It keeps work tied to the fixed-weight capability network.

Consult the [handoff record](docs/AGENT_HANDOFF.md) before changing an active workstream. Use feature branches and pull requests; keep provider credentials and raw runtime logs out of Git. Commit and push working increments so teammates on other machines can integrate them.
