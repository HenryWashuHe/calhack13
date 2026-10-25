# Domain-free iteration (experimental)

How a method gets better across generations without touching any model weights, in a way that
does not depend on the task domain and can be passed from one organization to another with its
evidence. This contract names the roles, the loop and the rules; `scripts/iteration/engine.mjs`
is its executable reference, exercised only on a synthetic family in `engine.test.mjs`. Nothing
here claims the loop ran on a real task family; binding it to the lab tools is the last section.

## Roles a domain must provide

| Role | What it is | Real-lab counterpart |
| --- | --- | --- |
| **Family** | `id`, `root()` (a baseline package source), `instances(seed, n)` (deterministic, hidden until frozen), `score(package, instance)` returning `{ success, progress, cost, detail }` with no randomness | `lab-task` + `lab-instance` generator; `lab-exec` then `lab-score` (`evaluation.json` → `scoring.instances[]`) |
| **Mutator** | `propose({ parent, feedback, k, generation })` → up to `k` `{ source, note }`; sees the parent and attributed feedback, never an instance id | `method-generate-step` / parent-bound revision with `lab-receiver-feedback/1` as input |
| **Controls** | fixed packages the candidate must beat (a fixed design, a hand-written baseline) | the fixed-cart and seeded random-search methods |
| **Criterion** | `success-count-margin` with `min_margin >= 1`, declared in the plan | `lab-criterion/1` `success-count-margin` |

The engine owns everything else: the frontier, the plan, the ledger and inheritance.

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
   fall to plan order. Rejections are recorded with the same detail as promotions.
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

## Known limits of v1

- A promotion is a margin on the same frozen instances. Counts from different generations are not
  comparable, so a frontier can move on a lucky draw; raise `heldout` or `min_margin` to resist
  noise, or add a second family as a check. The ledger makes any such move visible.
- One family per loop. Running the same candidates across several families and promoting only a
  package that wins on all of them is a policy on top of this contract, not inside it yet.
- The engine counts evaluations and mutator calls; it does not price them.

## Passing on (inheritance)

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
generator; `score` runs `lab-exec` and `lab-score` for one method on one instance and maps
`evaluation.json` `scoring.instances[i]` to `{ success, progress, cost }` and the submitted design
to `detail`. A `Mutator` adapter wraps a guarded generation step with the receiver feedback document
as input and returns the exported `MethodPackage` source. The plan the engine freezes maps onto a
`development-trial-plan/1` with one slot per arm and `success-count-margin`; its verdicts map onto
`verdict.json`. Costs stay where the lab accounts for them; the engine records counts, not money.
None of this binding exists yet; it is the forge lane's call whether to host it there or let this
engine drive the CLI.
