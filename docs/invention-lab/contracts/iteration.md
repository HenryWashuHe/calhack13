# Domain-free iteration (proposed, exploratory)

**Status: a proposal under interface review.** This document and `scripts/iteration/engine.mjs`
describe one way a method could get better across generations without touching any model
weights, independent of the task domain, and be passed from one organization to another with its
evidence. The reference engine is an executable spec for discussing the algorithm, exercised only
on a synthetic family in `engine.test.mjs`. It is **not** a second orchestrator, its ledger and
score objects are **not** substitutes for the retained contracts named below, and nothing here
changes the invention-lab completion gates, authorizes live provider requests, or claims the loop
ran on a real task family. Integrating it as a runtime needs a shared-interface agreement first.

## Roles a domain must provide, and what they are on main

| Role here | What the reference expects | The retained contract it must bind to |
| --- | --- | --- |
| **Package** | `{ sha256, source, parent, origin, generation }` | `MethodPackage` with its exact dependency identities (`ExactContentRef`: domain, schema, id, version, sha256) from [packages](packages.md) and [identity-and-execution](identity-and-execution.md); the engine's `sha256(source)` is a stand-in for that identity, not a replacement |
| **Family** | `id`, `root()`, `instances(seed, n)` deterministic and hidden until frozen, `score(package, instance)` → `{ success, progress, cost, detail }` with no randomness | `lab-task` / `lab-instance` generator; **separate** execution and scoring records: a `DevelopmentRunBinding` + `ExecutionReceipt`, then an `EvaluationRequest` whose `Scored` outcome yields `scoring.instances[]` ([evaluation](evaluation.md)); the engine's `score` collapses two receipts into one call only in the synthetic reference |
| **Mutator** | `propose({ parent, feedback, k, generation })` → up to `k` `{ source, note }`; sees the parent and attributed feedback, never an instance id | a guarded generation step or parent-bound [revision](revision.md), admitted under [generation-admission](generation-admission.md) and a [finite chain allocation](chain-allocation.md) with its cost records; `lab-receiver-feedback/1` is the feedback, and it is **public development feedback including rejected runs**, not an accepted receiver decision; acceptance stays in the receiver's separately retained verdict and adoption path and is never inferred from feedback |
| **Controls** | fixed packages the candidate must beat | the fixed-cart and seeded random-search arms of a trial |
| **Plan** | arms, criterion, held-out instances, hashed before any score | a `TrialPlan` (`development-trial-plan/1`, [trials](trials.md)): ordered slots, one attempt and at most one retained output per slot, criterion `success-count-margin`, hidden-set commitment; the engine's `plan.sha256` is the analogue of the plan's exact identity |
| **Ledger** | append-only chain-hashed records of proposals, plans, evaluations, verdicts, promotions, adoptions | the retained receipts, `evaluation.json`, `verdict.json`, `ledger.json` and chain cost records already on main; the engine's ledger is a discussion aid and must not bypass them |

The engine owns the loop, the frontier and inheritance as described below; it owns none of the
identities, receipts or cost accounting above.

## The loop (one generation, one family)

1. **Incumbent.** The family's frontier package, or `root()` while there is none.
2. **Candidates before instances.** The mutator proposes from the incumbent and its attributed
   feedback (the per-instance `success`, `progress`, `cost`, `detail` of the incumbent's last
   evaluation). Candidates identical to the incumbent or a control, and duplicates, are dropped and
   recorded. No instance id exists yet.
3. **Freeze.** The plan fixes the arms (incumbent, every control, every candidate), the criterion
   and the held-out instances drawn from `sha256(seed:family:generation)`. Its hash is recorded
   before any score.
4. **Evaluate.** Exactly one evaluation per arm per instance, in plan order. No reruns, no
   selection among runs, no edits.
5. **Verdict.** Every candidate gets a verdict against the incumbent and every control: promotable
   only if its success count exceeds each by `min_margin`. The reason states every count compared.
6. **Promote at most one.** The best promotable candidate becomes the frontier; ties among the best
   fall to plan order. Rejections are recorded with the same detail as promotions, and a candidate
   whose comparison involves any unknown outcome is `insufficient`, which is neither.
7. **Stop rules.** A generation count, an evaluation or call budget (checked before a generation
   starts, never overrun), or a stall of N generations without a promotion. The reason is recorded.

## Rules the reference enforces

- **Append-only, chain-hashed ledger.** Every record carries its index, the previous head and its
  own SHA-256 over canonical JSON. `verifyLedger` detects any altered or reordered record.
- **First provenance wins.** A package is identified by the SHA-256 of its bytes; re-proposing known
  bytes never rewrites its parent. Lineage is therefore acyclic.
- **Determinism.** Same seed, family, mutator and controls give the same ledger head.
- **No hidden-set leakage.** Instances are drawn after candidates exist, from a seed the mutator
  never receives; the next generation's feedback is about the previous generation's instances, which
  are disclosed only after their plan was frozen and scored.

## What a frontier is, and is not

A frontier is a **scoped, receiver-owned selection**: the package this organization currently
prefers for this task distribution, under this criterion, these controls and this budget. It is not
a claim of universal verified superiority, and it is not transferable as a claim. Declining to
promote is a valid outcome of every generation; no task is ever required to promote something, and
a loop that ends with the root as its incumbent is a complete, honest run.

## Selection data versus a final fresh panel

Every held-out draw the engine makes during the loop is **selection data**: the moment a
generation's instances are scored, their results feed the next generation's attribution, so
repeatedly selecting against them leaks the panel into the method. The reference therefore draws a
new panel per generation, and `confirm` scores the frontier once more on a **final fresh panel**
drawn from a seed never used for selection and never fed back. A confirmation is recorded, never
used to choose anything, and is the only number a frontier claim should quote. In the lab this is
the separately held hidden-scoring path (`HiddenCommitment` in [trials](trials.md)), which this
reference does not replace.

## Reference behaviour fixed under review

The reference now enforces, with the reviewers' probe files retained byte-identical beside its own:
a refusal that spends nothing when `k` or `heldout` is not a positive integer or the family returns
a panel that is not exactly the declared size of unique ids; strict boolean `success`, where any
other value is an unknown outcome and every verdict that would compare it is `insufficient`, never
promoted or rejected; a scorer that throws is recorded as a failed evaluation that counted, the
plan and completed arms are retained in `pending`, and the next generation for that family resumes
that plan instead of drawing another; the ledger is a canonical hash chain (sorted keys, explicit
`-0`, `Infinity` and `NaN`) whose declared head must equal the recomputed one, so truncation,
reordering and value edits are all detected; every record carries the lab; held-out seeds include
the lab, so an adopter never re-verifies on the sender's panels; the root package is registered and
appears in lineage; feedback carries per-instance results by index, never instance ids; adoption
checks that the package was promoted by the sender in the bundle's own ledger and that every
lineage entry and parent link is one that ledger recorded, declines otherwise, records every
outcome, and never rewrites a package the receiver already knew; `confirm` is declared before it
scores, once per frontier, on a seed never used before, on instances disjoint from every selection
and confirmation panel, within an optional evaluation budget.

## Known limits of v1

- A promotion is a margin on the same frozen instances. Counts from different generations are not
  comparable, so a frontier can move on a lucky draw; raise `heldout` or `min_margin` to resist
  noise, add a second family as a check, and quote only the confirmation.
- One family per loop. Running the same candidates across several families and promoting only a
  package that wins on all of them is a policy on top of this contract, not inside it yet.
- The engine counts evaluations and mutator calls; it does not price them and does not replace
  the chain allocation and cost records.

## Passing on (inheritance)

Transfer preserves the receiver's **independent choice** (it may decline), **pinned versions** (the
exact package and dependency identities travel, nothing floats) and **authority** (adoption is an
act of the receiving organization under its own constitution and budget, never of the sender).

`exportBundle` packs the frontier package, its ancestry (hashes, origins, generations), the whole
ledger and the ledger's hash. `adopt` on another organization first verifies the bundle's ledger
and the package hash, then runs one generation with that package as the only candidate against the
adopter's own incumbent and controls on the adopter's own instances. The package becomes the
adopter's frontier only on a local win; the adoption and its verdict are on the adopter's ledger
with the sender's identity and ledger hash. The sender's numbers are never trusted, only carried.

This is the same discipline as the kernel's capability inheritance: exact identity, the receiver's
own acceptance, provenance kept.

## What today's real evidence is, in these terms

The runs under `docs/invention-lab/evidence/live-method-20261024-*` are this loop with `k = 1`,
no frontier retention across families, feedback carried by hand between organizations and plans
written by people. Both generations were rejected against seeded random search; the record kept
them. Moving from that protocol to this engine changes four things: `k > 1`, a per-family frontier,
attributed feedback fed back automatically, and declared budgets with stop rules.

## Binding to the lab tools

A `Family` adapter wraps the retained CLI: `instances` lists `lab-instance/1` refs for a seeded
generator; `score` is a per-instance **view** derived from two retained records, one frozen method
run over its whole development panel (`lab-exec` once per method, yielding the run binding, receipt
and transcript) and the separate output scoring of that run (`lab-score`, `evaluation.json`,
`scoring.instances[i]` → `{ success, progress, cost }`, submitted design → `detail`). The engine's
one-call-per-instance shape never means one executor invocation per instance. A `Mutator` adapter wraps a guarded generation step with the receiver feedback document
as input and returns the exported `MethodPackage` source. The plan the engine freezes maps onto a
`development-trial-plan/1` with one slot per arm and `success-count-margin`; its verdicts map onto
`verdict.json`. Costs stay where the lab accounts for them; the engine records counts, not money.
None of this binding exists yet; it is the forge lane's call whether to host it there or let this
engine drive the CLI.
