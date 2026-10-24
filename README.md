# GENESIS

**A self-healing operating system for networks of AI agents.**

GENESIS provides infrastructure for agents to **build, verify, share, inherit, and improve executable capabilities** across independent organizations. A capability developed for one mission can become the starting point for another. A verified repair can become a reusable improvement instead of knowledge trapped in one agent's conversation.

The long-term goal is **networked recursive capability improvement, with fixed model weights**. What evolves is the system around the model: tools, workflows, operational knowledge, specialist teams, and eventually the methods agents use to build and repair those capabilities.

> Agents can build on the network's applicable experience—including better ways to build, verify, and repair.

## How the network improves

1. **Discover:** An agent encounters a requirement and searches for an existing capability that fits it.
2. **Organize:** If the capability is missing, the organization assigns responsibility and can create or recruit the specialists needed to develop it.
3. **Build and verify:** Specialists produce an executable candidate. Independent checks establish its tested behavior, artifact identity, permissions, and limitations.
4. **Publish and inherit:** The verified version enters a shared registry. Another organization discovers it and runs its own acceptance checks before adoption.
5. **Repair and build further:** Failures should lead to bounded, evidence-backed repairs. Useful capabilities should also become dependencies of new capabilities, including better builders and repair workflows.

Organizations are the authority boundary: each owns its missions, roles, budgets, grants, and adoption decisions. The shared registry connects independently operated organizations. Receiving a published capability does not automatically authorize its execution.

GENESIS supports two connected forms of evolution: **organizational evolution**, changing who is responsible and which specialists participate; and **capability evolution**, changing what those specialists can execute and share.

## What “recursive” means here

A growing capability library provides accumulation. Reusing or composing its contents can improve a task. The recursive step is an improvement that makes the system **better at producing its next improvement**, demonstrated on independent evaluation.

For example, one organization publishes an extractor; another develops a regression-test generator; a third uses both to improve a tool-building workflow. If the first organization can then adopt that workflow and build better tools on new tasks, the loop has closed. The trusted acceptance criteria remain independent of the component being improved.

This is the intended architecture. **The current implementation establishes important foundations; it does not yet demonstrate a general recursive self-improvement loop.** No model-weight training is required by this design, and there is no claim of autonomous model-weight improvement.

### Universal infrastructure, selective transfer

A universal runtime does not require universal tools. An equation solver may help a later equation or a function plot and contribute nothing to drawing a cartoon cat. A capability can become useful many missions later; consecutive tasks need not share it. These are execution and evaluation tasks, not model-training steps.

GENESIS should retain useful experience, retrieve candidates, check applicability, and adopt only under the receiving organization's policy and independent acceptance evidence. **Finding no suitable capability is a normal outcome.** Reuse, composition, adaptation, fresh creation, and continuing with existing tools are all legitimate choices. A failed or unavailable search is an unknown outcome, not evidence that no suitable capability exists.

Tools, domain workflows, and methods for building or repairing tools have different scopes. Workflow structure may transfer while its evaluator does not: checking an equation and judging a drawing require different evidence. More stored capabilities do not guarantee higher quality, and even a rejected irrelevant tool can add retrieval cost. Tests reduce uncertainty; they cannot guarantee correct selection or eliminate negative transfer.

The research goal is measurable improvement on a declared mix of tasks, with failures and costs reported—not improvement after every task. See the [recursive capability improvement goal](docs/RECURSIVE_CAPABILITY_IMPROVEMENT.md) for implementation milestones, selective-transfer cases, and controlled evaluation.

## What the current implementation supports

| Mechanism | Current support and boundary |
| --- | --- |
| Organizational evolution | The Rust kernel supports role creation/retirement, specialist recruitment, responsibility assignment/delegation, and capability attachment. Proposals pass authority and lifecycle checks before committing. |
| Executable capability creation | The acquisition workflow generates TypeScript extraction candidates, packages source-bearing Apify Actors, pins builds/runs, and verifies outputs against an independently configured suite. The implemented executable artifact path is specialized to Apify Actors. |
| Bounded repair | Acquisition can perform one planned semantic repair after a fully collected rejection, preserving lineage and rerunning verification. This is not yet a general repair service for deployed capabilities across the network. |
| Cross-organization inheritance | Immutable versioned manifests, shared discovery, adopt-before-acquire, and organization-local acceptance are implemented. A two-organization integration exercises A creating/publishing a capability and B adopting it without B synthesizing another copy. |
| Persistent specialist work | Fetch supplies independent signed specialist exchanges; CREAO supplies persistent operational agents; Elastic supports organizational evidence, retrieval, and Agent Builder integrations. End-to-end live sponsor evidence remains a separate gate. |
| Recovery and control | SQLite journals, exact artifact/request identities, authority checks, bounded attempts, and reconciliation states protect the tested workflows. Ambiguous external effects can require operator attention; recovery does not mean every operation can safely be replayed. |
| Recursive composition and network healing | Typed capability dependencies, executable builder/repair packages, shared failure advisories, controlled upgrade/rollback, and measured improvement of the improvement process remain next milestones. |

A manifest already carries schemas, permissions, provenance, verification evidence, and optional role/workflow descriptions. Those descriptions do not yet constitute a general executable workflow package or dependency resolver. The network currently uses a shared registry service; it is not a decentralized peer-to-peer capability protocol.

Verification establishes behavior within the tested scope. It does not prove arbitrary generated code is safe, correct on every future input, or suitable for another organization's mission without that organization's checks.

## The demonstration

**Current reproducible beat:** organization A requests a missing capability; its workers acquire, verify, publish, and locally accept it. Independent organization B requests the same need, discovers A's version, runs its own checks, and installs it with no B synthesis. The default rehearsal seeds the requests and uses explicitly labeled loopback model/provider responses.

**Next demonstration:** break a previously working capability. A detects the failure, assigns repair responsibility, recruits specialists when needed, publishes a verified replacement, and resumes. B independently checks and adopts the repair. Then show one testing or repair capability developed through GENESIS and inherited by another organization improving the next build on an unseen task, with the same base model and comparable budget. That final step is the evidence needed for the recursive claim.

Include three transfer cases: a new input that justifies direct reuse; a related task that benefits from composition or a development workflow; and an unrelated requirement where the old tool is correctly left unused. Score task outcomes as well as selection: avoiding a tool while failing a solvable task is not success. The current Actor path supports a narrow web workload; broader math or drawing demonstrations require additional executors and domain-specific evaluators.

The venue scenario is the current reference workload: extraction, monitoring, disruption, and replacement selection exercise the infrastructure with observable inputs and outcomes. GENESIS's product is the capability network and evolving organization runtime; additional workloads should use the same mechanisms.

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

## The most valuable next improvements

1. **Close one network repair loop.** Persist a failure against an exact capability version, open a bounded repair intent, independently verify a replacement, and let another organization adopt it under its own policy. Keep uncertain effects in reconciliation; make withdrawal and rollback explicit rather than silently replacing installed versions.
2. **Make capabilities composable.** Add typed, pinned dependency and derivation relationships, then an executable contract for reusable workflows such as builders, test generators, and repair strategies. Preserve existing verification and permission boundaries as artifact types expand.
3. **Measure selective and recursive benefit.** Compare the same base system without cross-task accumulation, with task capabilities, and with task capabilities plus inherited development workflows on held-out tasks. Freeze the model and trusted evaluation, count creation/verification/retrieval costs as well as execution, report regressions by task family, and gate promotion on criteria fixed before evaluation. Generated tests can supplement the evaluation; they cannot replace its trusted acceptance criteria.

These are proposed implementation priorities, not completed features. Live sponsor validation proceeds alongside them; neither a passing fixture nor a larger capability count alone establishes recursive improvement.

The [documented goal and acceptance criteria](docs/RECURSIVE_CAPABILITY_IMPROVEMENT.md#goal-and-completion-evidence) turn these priorities into a reviewable implementation target. They do not change active workstream ownership.

## Sponsor responsibilities

| Platform | Contribution to the system |
| --- | --- |
| **Fetch.ai** | Discovery, communication, and recruitment of independently running specialists. |
| **Elastic Agent Builder on Serverless** | Organizational intelligence, specialist agents and skills, evidence retrieval, and searchable shared capabilities. |
| **Apify** | Creation, build, and execution of missing web capabilities as software Actors. |
| **CREAO** | Reusable operational agents and persistent specialized responsibilities. |
| **GENESIS** | Organization structure, authority, budgets, lifecycle, capability identity, verification, and adoption decisions. |

[Apify sponsor notes](docs/APIFY_SPONSOR_NOTES.md) explain meaningful-use evidence and the separate AI Chat category. [CREAO sponsor requirements](docs/CREAO_SPONSOR_NOTES.md) track actual core agent work, agent ID, and a genuine run share link. [Fetch sponsor requirements](docs/FETCH_SPONSOR_NOTES.md) track ACP, Agentverse discovery, and actual ASI:One use. Local signed-process results, loopback tests, and cloud-provider evidence must remain distinguishable.

## Stack

| Layer | Technology |
| --- | --- |
| Organization kernel, API, scheduler, capability gateway | Rust, Axum, Tokio |
| Provider adapters and typed contracts | Rust, reqwest, serde |
| Authoritative local state | SQLite, transactional mutations, durable journals/outbox |
| Mission and organization interface | TypeScript, React |
| Generated Apify Actors | TypeScript with a fixed build scaffold |
| Fetch agent transport | Python uAgents sidecar behind a typed local API |

The Rust kernel remains the authority boundary. Elastic, Apify, and CREAO integrations use HTTP; the Fetch sidecar isolates the official Python SDK from the kernel. This framing does not require a language or runtime rewrite.

## Start here

- [Technical specification](docs/TECH_SPEC.md) and [research report](GENESIS-organization-evolution-research.md): architecture, original research, and design rationale. This README states the current product framing; older milestone/status prose in detailed guides should be read against the current handoff.
- [Capability execution](docs/CAPABILITY_EXECUTION.md), [adopt-before-acquire](docs/CAPABILITY_GAP_ADOPTION.md), and [automatic finalization](docs/CAPABILITY_GAP_FINALIZER.md): publication, independent acceptance, and inheritance.
- [Source synthesis](docs/CAPABILITY_SYNTHESIS.md), [durable acquisition](docs/CAPABILITY_ACQUISITION.md), [server-managed acquisition](docs/SERVER_ACQUISITION.md), and [forge integration](docs/FORGE_INTEGRATION.md): executable creation and bounded repair.
- [Inherited mission](docs/INHERITED_MISSION.md) and [Fetch venue recovery](docs/VENUE_RECOVERY.md): concrete cross-organization and specialist workflows, with their evidence limits.
- [Fetch runtime](docs/FETCH_RUNTIME.md), [CREAO runtime](docs/CREAO_RUNTIME.md), and [provider contracts](docs/PROVIDER_CONTRACTS.md): integration boundaries and setup.
- [Live gates](docs/LIVE_GATES.md), [handoff](docs/AGENT_HANDOFF.md), and [review queue](docs/PR_REVIEW_QUEUE.md): validation evidence, active ownership, and outstanding work.
- [Team workstreams](docs/TEAM_WORKSTREAMS.md) and [implementation split](docs/NEXT_IMPLEMENTATION_SPLIT.md): coordination history; consult the current handoff before taking ownership.

## Working together

Consult the [handoff record](docs/AGENT_HANDOFF.md) before changing an active workstream. Use feature branches and pull requests; keep provider credentials and raw runtime logs out of Git. Commit and push working increments so teammates on other machines can integrate them.
