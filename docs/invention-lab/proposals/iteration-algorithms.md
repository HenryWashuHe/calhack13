# Proposal: iteration algorithms (D)

**Status:** proposal — not a completion-gate claim.
**Scope:** `experiments/strategy-adapter/**`, `docs/invention-lab/proposals/**`
           (Eric's research lane).
**Builds on:** PR #144 (`claude/iteration-engine`), `claude/regression-analysis`,
               `docs/invention-lab/proposals/strategy-adapter.md`.

---

## 1. Root causes from the evidence

Evidence sources (all local simulation; no model calls):

| Cause | Where established |
|---|---|
| C1: wrong-family feedback | A report §3; `claude/regression-analysis` |
| C2: strategy prescribed in the prompt | A report §2 |
| C3: population of one | A report §1 |
| C4: 6 instances, margin 1 | A report §4; `claude/regression-analysis` |
| C5: under-tested selection (`(s+0.3)/(t+1)` ranks 2/2 above 5/6) | `claude/regression-analysis` `bf832cd` |
| C6: blind adapter features (rough-transport `gap = step_height = 0`) | This branch; `docs/invention-lab/proposals/strategy-adapter.md` §5 |

---

## 2. What PR #144 already covers

PR #144 (`claude/iteration-engine`) implements:

- k candidates per round (k 4–6)
- Per-family frontier tracking
- Regenerated candidate panels each round
- Final fresh confirmation run
- Chain-hashed ledger
- Plan and verdict emitters

This document does not re-specify those. Each option below says what it adds
on top of #144.

---

## 3. Options

### A. Per-family μ+λ frontier (recommended first)

**What it adds to #144:** explicit (μ, λ) selection — keep the μ best strategies
on each family's frontier, generate λ new ones per round by mutating those μ
parents.

**Causes addressed:** C3 (population grows to μ), C5 (selection rule is explicit
and separates wins from progress), C6 (strategies and adapters are now separate
files — `hardness` in `adapter.py` can be mutated independently).

**With the split:** mutation may target `strategy.py` alone, `adapter.py` alone,
or both. Strategy migration across families (option C) uses strategy-only variants.

**Metrics (predeclared):**
- Per-family frontier success over rounds (fraction of frontier members that pass
  the family's acceptance check).
- Families where the frontier beats seeded random search (same budget, same
  evaluator, random designs sampled from the parent's `AXES`).

### B. Quality-diversity archive (A's storage)

**What it adds to A:** a 2-D archive indexed by `(family, strategy-behavior cell)`.
Strategy-behavior is a compact descriptor of the `strategy.py` component: e.g.,
`(allocate_style, stop_style)` as a small categorical vector.

**Causes addressed:** C4 (the archive retains diverse strategies across families,
so a 6-instance family with margin 1 contributes one cell rather than dominating
the frontier).

**When to use:** implement B as A's storage layer once A is stable (at least one
family's frontier beats seeded random search).

### C. Islands with strategy migration

**What it adds to A:** separate island populations per family; periodically migrate
the best `strategy.py` from one island to another, keeping each island's
`adapter.py` fixed.

**Causes addressed:** C1 (an island receives feedback from exactly one family's
evaluator; cross-family feedback comes only through migration, not through mixed
training).

**With the split:** migration is well-defined because `strategy.py` and
`adapter.py` are separate files. A migrated strategy runs with the destination
island's adapter unchanged.

**Predeclared metric:** families where a strategy wins outside the one it was
first introduced in (i.e., a strategy originating on family X later passes
acceptance on family Y without having been trained on Y).

**When to use:** after A is stable on at least two families.

### D. Bandit allocation + successive halving

**What it adds to A or B:** a two-level budget allocator. Upper level: a
multi-armed bandit chooses which family island to run next. Lower level:
successive halving within a round eliminates weak candidates early, giving the
remaining budget to the survivors.

**Causes addressed:** C4 (successive halving reduces wasted evaluations on the
6-instance family), C5 (the bandit tracks per-family empirical win rates and
reallocates budget to families where the frontier is improving).

**When to use:** after A + B are stable; adds complexity that is only warranted
once the basic frontier is running.

### E. Recursive: evolve the mutator

**What it adds:** archive the mutator prompt and feedback format as versioned
files; apply the same A+B loop to mutator variants (a mutator that produces
better strategies faster wins).

**Causes addressed:** C2 (the strategy is no longer prescribed; the mutator's
own selection logic is evolved).

**When to use:** only after A beats seeded random search on two families. E
requires a working outer loop to evaluate mutator variants reliably; running it
before that baseline exists conflates mutator noise with strategy noise.

---

## 4. Recommendation

Implement in this order:

1. **A first:** per-family μ+λ frontier on top of #144. This directly addresses
   the largest causes (C3, C5, C6) with the least added complexity.
2. **B as A's storage:** once A produces results, add the quality-diversity
   archive as a drop-in replacement for A's frontier data structure.
3. **C after A is stable:** strategy migration once at least two families have
   working frontiers.
4. **D after B is stable:** bandit + successive halving once the basic frontier
   shows the per-family signal is meaningful.
5. **E only after A beats random search on two families:** recursive mutator
   evolution is the moonshot step; running it earlier produces uninterpretable
   results.

---

## 5. Predeclared metrics (all options)

No synthetic curves. Every reported number cites a file on `main` or on the
branch being evaluated.

| Metric | Definition |
|---|---|
| Frontier success rate | Fraction of frontier members passing the family's acceptance check, per round. |
| Beats-random threshold | The frontier's best result exceeds the 90th percentile of 100 random designs (same budget, same evaluator, designs from the parent's `AXES`). |
| Cross-family win | A strategy that first passed acceptance on family X later passes acceptance on family Y (C migration metric). |

---

## 6. Hard limits

- No forge, executor, kernel, console or bridge files without an accepted claim on issue #1.
- No model calls; all evidence is local simulation.
- Synthetic results are never presented as real evidence.
- Every number in a document cites a file on `main`.
