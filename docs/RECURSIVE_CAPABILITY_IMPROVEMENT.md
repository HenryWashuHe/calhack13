# Recursive capability improvement: goal and design

Status: proposed implementation and evaluation target. This document records the goal requested in the project-framing discussion; it is not an app-scheduled task, a completed experiment, or an ownership reassignment. Consult [AGENT_HANDOFF.md](AGENT_HANDOFF.md) before implementation. The current implementation boundary is summarized below.

## Goal and completion evidence

**Enable independent organizations to selectively inherit verified capabilities, repair failures, and reuse development capabilities to improve subsequent capability creation or repair, while keeping model weights fixed.**

The moonshot is a network whose useful experience compounds across organizations, including experience about how to produce further improvements. Universal infrastructure means admitting different domains and their evaluators; it does not mean every tool helps every task.

The implementation target is complete when the following evidence is reviewable:

- [ ] A records a failure against an exact installed version, performs a bounded repair, verifies and publishes a distinct replacement, and resumes its mission. Ambiguous effects enter reconciliation.
- [ ] B independently discovers, verifies, and adopts that replacement using its own state, grants, and inputs. Its installation remains pinned; publication alone never upgrades it.
- [ ] A published development capability is executable, has pinned dependencies and provenance, and is actually used by another organization to build or repair a later capability. Its use is visible in execution evidence, not inferred from a description.
- [ ] Direct reuse, related transfer/composition, and an irrelevant-capability case all have reproducible traces with independently scored task results. The irrelevant capability is not forced into execution.
- [ ] The controlled evaluation below produces per-family quality, cost, latency, intervention, and regression results, including unsuccessful runs. Provider fixtures and authenticated live runs are labeled separately.

These gates complete the mechanism and evaluation milestone. A **positive recursive-improvement claim** additionally requires the development-workflow treatment to meet quality and cost criteria fixed before the independent evaluation. If it does not, report the result and leave that research claim open. There is no promised improvement after every task.

## What can transfer

| Retained capability | Plausible use | Boundary |
| --- | --- | --- |
| Task-specific executable | Reuse a portal extractor on new supported inputs | A different schema or site can invalidate it |
| Domain method | Combine an equation solver with a plotter | It need not help draw a cartoon cat |
| Development workflow | Reuse dependency setup, error diagnosis, or a repair strategy | Runtime, permissions, failure type, and evaluation criteria still matter |
| Verification procedure | Reuse the structure of generating candidates and checking outputs | A mathematical correctness check cannot establish artistic quality |

Transfer can occur many tasks later. Unrelated tasks between uses are expected. Saving an artifact is accumulation; using it is reuse; composing it with others is composition. Using retained experience to improve subsequent creation, verification, or repair supports the recursive claim only when the corresponding improvement is measured.

The system can learn at the system level without weight updates. Call these execution, development, and evaluation tasks; calling every run a training task would imply a model-training step that does not occur.

## Current foundations and gaps

| Area | Existing foundation | Next work |
| --- | --- | --- |
| Capability contract | [CapabilityManifest](../crates/genesis-core/src/capability.rs) carries applicability, limitations, schemas, permissions, artifact identity, verification, and provenance | Structured preconditions, versioned environment/evaluator evidence, observed costs, and typed dependencies |
| Independent inheritance | [Adopt-before-acquire](CAPABILITY_GAP_ADOPTION.md) searches the registry and uses receiving-organization acceptance | Explicit task-fit decisions and evaluation of appropriate non-reuse beyond configured capability rules |
| Creation and repair | [Acquisition](CAPABILITY_ACQUISITION.md) builds extraction Actors and permits a bounded semantic repair | Deployed-capability failure handling, shared advisories, and independently adopted replacements |
| Development knowledge | Manifests can carry workflow descriptions | Executable builder/test/repair packages and evidence that they improve later development |
| Organization changes | Roles, recruitment, delegation, retirement, and capability attachment | Use structural changes when new responsibility warrants them; reuse an existing owner when sufficient |
| Evaluation | Two-organization inheritance tests and [planner evaluation](PLANNER_EVAL.md) exercise specific mechanisms | Controlled capability-transfer and recursive-improvement evaluation; existing fixtures do not establish this claim |

The current executable path is specialized to TypeScript web extraction on Apify. A general math-and-drawing demo needs additional artifact/executor contracts and appropriate evaluators. Describing them in a manifest does not implement those runtimes. The shared registry is the present network mechanism, not a decentralized protocol.

## Adoption with evidence

Extend the existing manifest and adoption receipts; do not introduce a parallel registry. Proposed additions must remain clearly separate from fields already implemented:

- Preconditions: supported task/input families, runtime and dependency requirements, domain restrictions, and evaluator identity/version.
- Evidence: tested cases and environments, known failure classes, timestamps, relevant drift, and independently recorded outcomes. Descriptions are claims until checked; raw private fixtures and credentials stay local.
- Lineage: exact dependency versions/digests, derived-from and repairs relationships, and the development workflow that produced the artifact.
- Economics: observed creation, verification, invocation and retrieval cost, with measurement context rather than a universal fixed estimate.

A bounded discovery step returns candidates. Check canonical identity, authority, schemas, preconditions, dependency compatibility, and relevant evidence, then run organization-local acceptance when policy permits. Record why a candidate was selected or rejected. Rank similarity is not proof of suitability.

Distinguish `applicable`, `not_applicable`, `insufficient_evidence`, `no_candidate`, and `operational_unknown` as proposed decision semantics, not existing wire enum values. A no-match result can justify another approved path, including current tools or fresh creation. Incompatible or untested candidates may need adaptation and new verification. Registry failure or ambiguous execution must not be treated as an empty search or permission to duplicate an external effect. Preserve the current conservative fallback contract until a reviewed policy explicitly adds other paths.

Finite tests can miss failures. The enforceable promise is that required evidence and policy checks run and their outcomes are recorded; perfect applicability prediction and zero negative transfer are not guarantees. Even unused candidates consume retrieval/context budget. Keep search bounded, filter stale or deprecated candidates, and archive without erasing provenance or records needed by pinned installations.

## Implementation sequence

1. **Close the network repair loop.** Persist exact-version failure evidence and repair intent; distinguish semantic failure from transport uncertainty; independently verify a replacement; publish its lineage; let B accept or decline it. Add explicit deprecation/withdrawal and rollback policy without overwriting immutable versions or silently migrating installations. Rollback cannot undo an already executed external effect.
2. **Package one development capability.** Start with one bounded repair or build workflow using the existing Actor scaffold. Define typed inputs, outputs, exact dependencies, resource limits, evaluator requirements, and derivation evidence. Detect dependency cycles and prevent inherited workflows from expanding grants or replacing trusted acceptance rules. Generated tests supplement independent checks; they cannot self-certify quality.
3. **Exercise selective transfer and measure the extra benefit.** Add the three cases below and the evaluation arms. Start within the supported web runtime; expand domains only when executors and evaluators exist. Promote a candidate workflow only against predeclared criteria; otherwise retain the previous accepted version and report the failed experiment.

These steps preserve the four sponsor roles: Fetch recruits independent specialists, Elastic supplies organizational evidence and capability discovery, Apify builds/runs executable web capabilities, and CREAO owns continuing operational work. GENESIS owns authority, identity, lifecycle, verification and adoption. Changing the research framing does not establish live provider evidence or alter sponsor submission requirements.

## Smallest convincing transfer demonstration

| Case | What to show | What it establishes |
| --- | --- | --- |
| Direct reuse | B uses A's accepted extractor on a new supported input, with its own checks and no synthesis | Cross-organization reuse within the documented scope |
| Related but different | B uses an inherited repair/build workflow on a held-out portal variation requiring a new extractor | Process transfer; improvement still needs the controlled comparison below |
| Unrelated requirement | A later task uses other available capabilities, or creates a suitable one, while leaving the extractor unused | Appropriate non-reuse, with successful task completion scored separately |

For an initial web-only demo, the third task can be an unrelated local planning calculation using an already available, tested execution path. This is a selection control, not evidence of universal domain coverage. The original web extractor might be useful again much later.

The interface should show the requirement, candidate scope, selection/non-selection reason, exact executed versions, local acceptance, dependency lineage, and outcome/cost. A rising capability counter or a growing organization graph alone is not an improvement metric. Dynamic roles remain tied to accountable work rather than spawning for spectacle.

## Controlled evaluation

Use three treatments to separate different explanations for a gain:

| Treatment | What persists between tasks |
| --- | --- |
| Baseline | Same initial tools and fixed development workflow; task-local execution/repair allowed, newly developed capabilities do not persist across tasks |
| Capability accumulation | Baseline plus retained task/domain capabilities available through the registry |
| Development accumulation | Capability accumulation plus retained executable builder/test/repair workflows |

Compare capability accumulation against baseline for reuse benefit; compare development accumulation against capability accumulation for the incremental benefit of improving development. An additional optional control can use fixed, human-authored development workflows to distinguish learning from simply adding another tool.

Before evaluation, fix the task families, evaluator/rubric versions, quality thresholds, permitted regression margins, budgets, and promotion criteria. Include repeated related tasks, changed inputs, delayed reuse, irrelevant capabilities, and environmental drift. Hold model identifier/version, model settings, initial tools, permissions, and infrastructure constant; version the prompts and workflow differences that form the intended treatment. Report provider/model changes that prevent exact matching.

Keep development/selection tasks separate from final held-out evaluation. Builders must not see hidden expected outputs or modify the evaluator. Freeze the capability library before the final test, or use a declared online protocol where each task may access only the earlier task prefix. Do not rerun against the same exposed holdout until a favorable candidate appears. New independent evaluation is needed after further tuning.

Use paired task sets, recorded seeds where supported, and multiple task orders/repetitions. Seeds do not guarantee deterministic provider behavior. Report counts and uncertainty; a single curated chain establishes mechanics only. Start with heterogeneous tasks within supported executors and name that scope precisely. Claims of broader transfer require broader evaluated domains.

Measure:

- Task success/quality and human intervention, using each domain's independent criteria. Keep schema validity separate from substantive quality; aesthetic tasks need an explicit rubric or blinded assessment.
- Creation and repair success on held-out development tasks, attempts needed, and time to an accepted capability.
- Correct and incorrect adoption, appropriate non-reuse, failures after adoption, rollback/reconciliation events, and search overhead.
- End-to-end latency, model usage, provider work, and total cost, including discovery, synthesis, verification, failed candidates, and retained infrastructure where relevant. Report cold cost and amortized cost at a stated number of uses alongside warm invocation cost.

Use the same declared total resource budget for the primary comparison; report completed tasks and failures when a budget is exhausted. Per-task/warm comparisons are useful secondary results but cannot hide the cost of creating the inherited library. Present per-family results and regressions alongside aggregates; gains on repeated extraction tasks cannot stand in for gains on unrelated tasks.

Passing fixture runs verifies integration mechanics. Authenticated cloud runs verify provider execution. Positive results on this evaluation support improvement only for the tested task distribution and conditions, not monotonic or universal progress.

## Research basis and design inference

- [Voyager (2023)](https://arxiv.org/abs/2305.16291) uses an executable skill library and black-box model calls without parameter fine-tuning, and reports skill reuse in new Minecraft worlds. It supports the feasibility of accumulation and transfer within a related environment, not arbitrary cross-domain transfer.
- [Automated Design of Agentic Systems (2024; revised 2025)](https://arxiv.org/abs/2408.08435) searches for agent programs using an archive of previous designs and reports transfer experiments across domains and models. It supports investigating learned agent/workflow designs; it does not validate GENESIS's network or guarantee each discovered capability will transfer.

The adoption rules, three-treatment experiment, and implementation sequence above are GENESIS design proposals informed by these distinctions. Neither paper establishes that storing more tools alone causes recursive improvement. GENESIS's proposed contribution is the infrastructure for independent organizations to exchange executable improvements with local authority, verification, provenance, and measured applicability.
