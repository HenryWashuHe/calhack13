# Iteration algorithms on top of the proposed iteration contract (proposal)

**Status: a proposal in Eric's research lane**
([claim](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6391308515)).
It builds on the iteration contract and reference engine proposed in PR #144, read at head
`fd35c2f`, which is itself an unmerged proposal under review. It does not re-specify #144. It
evaluates five ways to extend it and recommends an order. Nothing here is implemented, no model is
called, and no curve, synthetic or otherwise, is presented as a result. The metrics at the end are
pilot research outcomes; they do not substitute for the canonical G4 comparisons or the accepted
A → B → A chain (`docs/ONGOING_GOAL.md`, "Direction update, October 24, 2026 (evening, Pacific)").

## The causes any option has to answer

| # | Cause | Where it is recorded on `main` | Layer |
| :-: | --- | --- | --- |
| 1 | Feedback from one family used to revise for another | `docs/invention-lab/analysis/why-generation-2-regressed.md` §3 item 4 | outer loop |
| 2 | A search strategy prescribed in the revision prompt | same report, §3 item 2 | mutator input |
| 3 | Population of one per generation | same report, §3 item 5 | outer loop |
| 4 | Six instances with a margin of one | same report, §3 item 5; `…/b-revision-receiver/vs-parent/criterion.json` | outer loop |
| 5 | Under-tested selection: `(s + 0.3) / (t + 1)` ranks 2 of 2 above 5 of 6 | same report, §2 and §3 item 1 (the one observed mechanism) | inside the method |
| 6 | A blind adapter feature: the difficulty order sorts on fields that are 0.0 on rough-transport | same report, §1; `experiments/invention-lab/evaluator/src/family.rs` lines 110 to 111 and 137 to 151 | inside the method |

Cause 6 is "finding 1" of the strategy/adapter proposal (`strategy-adapter.md`, PR #182). Causes 2,
3 and 4 are plausible contributors, not established causes; only cause 5 is observed in the retained
records (the report's own grading).

## What #144 already covers

Read from `docs/invention-lab/contracts/iteration.md` and `scripts/iteration/engine.mjs` at
`fd35c2f`:

- **Several candidates per generation.** `k` candidates (default 4) are proposed from the incumbent
  and its id-free attributed feedback before any instance is drawn. This is a (1 + λ) loop: λ = k
  candidates, but only one parent.
- **Per-family frontier, incumbent as an arm.** Every candidate must beat the incumbent and every
  control by `min_margin` on one frozen panel. At most one is promoted, and an unknown outcome makes
  a verdict `insufficient`. A candidate that replaces a working strategy or grid is judged against
  what it replaced. That is the outer-layer answer to the report's §3 item 3 as well.
- **Feedback stays within its family.** The frontier and its feedback are keyed by family, so the
  engine never carries feedback across families. That addresses cause 1 at the outer layer.
- **Regenerated panels and a final fresh panel.** Each generation draws `heldout` new instances
  (default 6), and `confirm` scores the frontier once on a disjoint panel never used for selection,
  within a budget. This makes cause 4 a parameter rather than fixing it: counts from different
  generations are not comparable, and a promotion is still a margin on one small panel (the
  contract's "Known limits of v1").
- **Budgets, stall, ledger, adoption.** Generation, evaluation and call budgets are checked before a
  generation starts. The ledger is chain-hashed. Adoption re-verifies on the adopter's own panels.

By its own text #144 does **not** cover: more than one family per loop; pricing (it counts
evaluations, not cost); the content of the mutator's prompt (the mutator is opaque); or a method's
internal estimator. The regression report §4 states that last limit directly: the outer layer
cannot stop a method from submitting a partially tested design.

## Five options

Each option is described as what it adds to #144, which causes it answers, and what it would need.

### A. Per-family (μ + λ) frontier, λ = 4 to 6

Keep the μ best verified packages per family (μ of 2 or 3) instead of one incumbent. Each
generation proposes λ candidates spread across those parents. A candidate joins the frontier only
by beating the controls and the frontier member it would displace, by the declared margin, on one
frozen panel shared by every compared arm. With the split (`strategy-adapter.md`), each candidate
declares its target, strategy or adapter, and leaves the other file byte-identical. λ is divided
between the two targets, so a gain can be attributed to one part.

- **Adds over #144:** a population that survives across generations, so one lucky promotion does
  not erase every alternative; attribution of gains to strategy or adapter.
- **Answers:** cause 3 directly. Cause 2 partly: a strategy-targeted candidate that drops a working
  loop must beat a frontier that still contains the loop. Cause 6 partly: adapter-targeted
  candidates can change features without touching the loop.
- **Needs:** the frontier becomes a set; the promotion rule names the member displaced; the ledger
  records each candidate's target. In the lab an arm costs one development run plus scoring on the
  panel (#144, "Binding to the lab tools"), so a generation costs λ + μ + controls runs. It must be
  priced in the chain allocation before it is declared.

### B. Quality-diversity archive: family × strategy-behavior cells

A small MAP-Elites-style archive. One axis is the family. The other is a coarse strategy-behavior
descriptor computed from the retained transcript, never self-reported. Two examples: the share of
development instances the submitted design was tested on before submission, and experiments to
first success. Each cell keeps its best verified package; a candidate takes a cell only by winning
a predeclared trial against that cell's elite.

- **Adds over #144:** diversity of strategies rather than μ near-copies, and a behavior label on
  every frontier member.
- **Answers:** cause 2 strongly. A prompt that pushes one style cannot displace an elite of another
  style without beating it. Cause 3. Cause 5 only as visibility: the child's submission was tested
  on two of six instances (report §2), which a coverage descriptor would have labeled, not prevented.
- **Needs:** fixed, tiny descriptor bins, declared before any run. More cells means more
  evaluations, so it is useful as A's parent pool (A's storage), not as a separate loop.

### C. Islands with strategy migration: the transfer experiment

One island per family, each running A. Every M generations an island's best **strategy file**
migrates to the other islands. There it is paired with the receiving island's own adapter and
enters as one candidate on the receiver's own panel. That is #144's adoption path: the receiver
re-verifies, and the sender's numbers are carried, never trusted. Adapters never migrate.

- **Adds over #144:** the only option that measures transfer. It turns "feedback from family X used
  to revise for family Y" (cause 1) into "a strategy from X tested on Y under Y's own trial". Negative
  transfer then becomes a recorded rejection rather than a regression absorbed into a revision.
- **Answers:** cause 1, and it makes the third metric below measurable.
- **Needs:** A stable on at least one family; the split, so a strategy can travel without its
  adapter; migration events in each island's ledger with the sender's ledger hash, which #144's
  bundle already carries.

### D. Bandit allocation and successive halving

Within one generation, score all λ candidates on a small first rung, keep the better half on the
same instances, and give the survivors a larger fresh rung. Repeat to a final rung. On the final
rung the survivors, the incumbent and every control run on one full frozen panel, and **only that
rung can promote**. Across families, an optional bandit gives the next generation's budget to
families with a better promotion record, with a declared floor so no family is starved.

- **Adds over #144:** the evaluations saved by not scoring weak candidates on the full panel go to
  a larger final panel. That attacks cause 4 without raising the total.
- **Answers:** cause 4. Cause 5 at the outer layer: rung decisions compare candidates only on
  identical instances, nothing is promoted from a partial rung, and no rule ranks fewer trials
  above more. The outer loop must not repeat the child's estimator.
- **Needs:** the rung schedule, keep rule and bandit floor predeclared before any score. Each rung
  is an ordinary predeclared trial, and a survivor runs again on the next rung, because a lab arm is
  a whole development run plus scoring, not a per-instance call. Today #144's emitter produces one
  acceptance plan per candidate/control pairing. Rungs would need a declared sequence of such plans,
  priced in the chain allocation.

### E. Recursive: archive and evolve the mutator's prompt and feedback format

Treat the mutator's prompt template and the feedback format as a package of their own, with a
frontier of their own. Their fitness is the number of verified promotions their candidates earn
under A, per evaluation spent, across families.

- **Adds over #144:** the only option that acts on the prompt itself (cause 2 at its source) and on
  what feedback says (cause 1's input).
- **Answers:** causes 1 and 2, in principle.
- **Needs:** every fitness sample is a whole A run, so it is costly and noisy, and it needs model
  calls, which this proposal does not make. It is easy to overfit to a family or a panel.

## Evaluation

| Cause | #144 at `fd35c2f` | A | B | C | D | E |
| --- | --- | --- | --- | --- | --- | --- |
| 1 wrong-family feedback | covered (per-family feedback) | – | – | **tests transfer explicitly** | – | input side |
| 2 prescribed strategy | partly (incumbent arm) | partly | **yes** | – | – | **at the source** |
| 3 population of one | partly (λ > 1, one parent) | **yes** | yes | – | – | – |
| 4 six instances, margin one | parameter only | – | – | – | **yes, within budget** | – |
| 5 under-tested selection | out of scope (inner) | – | visible only | – | outer-layer guard | – |
| 6 blind adapter feature | – | partly (adapter target) | – | – | – | – |

No option prevents cause 5 inside a method. That remains the job of the predeclared trial, which
caught it (report §4), and of a strategy-targeted mutation that changes the selection rule as a
visible diff of `strategy.py`. Cause 6 is better served by the adapter lint proposed in
`strategy-adapter.md` (flag a feature that is constant across a family's public instances) than by
any search algorithm.

## Recommendation

1. **A + D first.** A supplies the population and the strategy/adapter attribution. D supplies a
   larger deciding panel at the same cost. Both are outer-layer changes to #144's loop and need no
   change to the executor or scorer.
2. **B as A's storage** once A runs: draw A's μ parents from behavior cells, not from the top μ.
3. **C after A is stable** on at least one family, where stable is declared in advance (for
   example, no frontier change over a fixed number of generations, with a confirmation panel
   recorded). With the split, C migrates strategies only.
4. **E only after A beats seeded random search on two families.** Until then, evolving the prompt
   evolves noise.

## Predeclared metrics

Declared now, before any run, for whichever options are run:

1. **Per-family frontier success over rounds.** For each family and round, the frontier's success
   count on #144's final fresh confirmation panel, with the controls on the same panel. Only
   confirmation numbers are quoted; selection panels are not.
2. **Families where the frontier beats seeded random search.** An `accepted` verdict in a
   predeclared trial on a fresh panel, with seeded random search as the control, as in the retained
   `vs-random-search` trials.
3. **Families where a strategy wins outside the family it was introduced in.** This needs C. A win
   counts only with identical strategy bytes and an accepted trial on the receiving family, under
   the capability rule in `strategy-adapter.md`.

Always reported alongside: evaluations, development runs, generation calls, simulation steps and
the chain cost records; every rejected and `insufficient` verdict. A round with no promotion, and a
loop that ends on its root, are results. Today the starting point is zero for metric 2: no retained
trial on `main` accepts a generated method against seeded random search
(`docs/invention-lab/evidence/live-method-20261024-a-return/README.md`, "Result").

## Dependencies

- **#144 (ACP lane and contracts lane):** the interface agreement this builds on. Nothing here
  changes #144's engine.
- **Forge / generation:** candidates with `target: strategy | adapter`; admission and chain
  allocation for λ > 1 per generation and for rung schedules.
- **Executor and scoring:** unchanged. Rungs and islands are ordinary predeclared trials.
