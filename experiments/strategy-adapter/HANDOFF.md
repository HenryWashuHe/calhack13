# Handoff to a cloud session: make the iteration loop a real optimizer (Eric's research lane)

Written 2026-10-24 by Eric's Cowork session, which had read-only GitHub and no
Rust toolchain. This file is a coordination note: keep it in its own commit and
**drop it from every PR**.

Branch `eric/strategy-adapter-split`, based on `origin/main` `7e89172`.
Repository rules are in `CLAUDE.md`; follow them (claim on issue #1 before
shared edits, focused PRs, never take another session's files).

## 1. Do not redo: already done by other sessions

| Brief item | Where | State |
| --- | --- | --- |
| A. why generation 2 regressed | `origin/claude/regression-analysis` (`bf832cd`), `docs/invention-lab/analysis/why-generation-2-regressed.md` | pushed; adds a cause the brief missed: the child's selection rule `(s+0.3)/(t+1)` ranks 2/2 above 5/6, so it submitted a design run on 2 of 6 instances |
| C. iteration contract + reference engine | PR #144, `origin/claude/iteration-engine` (`880f7bc`); reviews on `codex/review-iteration144`, `henry-claude/review-iteration144` | in review; already has k candidates, per-family frontier, regenerated panels, final fresh confirmation, chain-hashed ledger, plan/verdict emitters |
| E. rules in the engine | part of C | same |

Re-fetch and check these heads first; they may have merged or moved.

## 2. Done on this branch (B, offline part)

- `experiments/strategy-adapter/parent-split/`: parent `8215c367` split into
  `strategy.py` (order, propose, allocate, stop, search; no domain nouns),
  `adapter.py` (protocol, features, space, build; no search loop), `method.py`.
- `experiments/strategy-adapter/harness/`: stdlib port of the rough-transport
  generator and design normalization (checked against retained digests), a
  replay oracle for retained runs, a synthetic differential oracle, 8 tests.
- Results (all pass): the split reproduces the parent's retained run exactly
  (6 experiments, 5,205 steps, submission `2483f08f…`, retained score 6/6);
  the harness also reproduces the child's 32-experiment run; parent and split
  are byte-identical on 500 synthetic cases.
- No forge/executor/core change is needed: packages already hold up to 256
  files and the executor runs `method.py` in a directory containing all of them.
  Strategy and adapter hashes are derived (`sha256` of each file's bytes).

Findings to carry into the docs:

1. **The parent's difficulty order is a no-op on rough-transport.** It sorts by
   `gap + step_height` (ravine fields); both are 0.0 on all six rough-transport
   instances (`family.rs`: rough-transport never sets them). The parent's 6/6
   came from its first admissible grid point. Brief diagnosis 2 ("screen on the
   hardest instance") and the A report's §1 wording ("the instance it ranked
   hardest") should say the screen-then-confirm loop worked but the ranking
   feature was blind here. Tell the A author on issue #1 rather than editing
   their branch.
2. This is the first concrete case for the split: the transferable part (the
   loop) and the non-transferable part (which feature means "hard") were
   tangled in one file.

## 3. Next steps, in order

1. **Claim** on issue #1 (draft in §5). Scope: `experiments/strategy-adapter/**`,
   `docs/invention-lab/proposals/strategy-adapter.md`,
   `docs/invention-lab/proposals/iteration-algorithms.md`. Nothing else.
2. **Live acceptance** (cloud has cargo). Use `none` mode if namespaces need
   privileges the VM lacks, and say which mode ran:
   ```sh
   cd experiments/invention-lab/executor && cargo build -q
   B=target/debug/lab-exec; O=/tmp/split
   $B package ../../strategy-adapter/parent-split parent-split 0.1.0 method.py $O.package.json
   $B example-plan none 6 72000 60000 $O rough-transport $O.package.json 0
   $B run $O/plan.json candidate $O.package.json $O/candidate none
   (cd ../scoring && cargo run -q --bin lab-score -- $O/plan.json candidate $O/candidate $O/candidate-score)
   $B check $O/plan.json candidate $O/candidate $O/candidate-score
   ```
   Expect 6 experiments, 5,205 steps, submission `2483f08f…`, 6/6. Do not
   touch executor files; report results in the PR, labeled as an author-run
   local process, not retained evidence.
3. **Spec B**: `docs/invention-lab/proposals/strategy-adapter.md`. Interfaces
   as implemented; identity (package hash unchanged; two derived hashes as a
   sidecar until Codex decides on a contract change); what a mutator may
   change in each file; the acceptance rule (replay-identical, then live);
   finding 1; the rule that a strategy is called a capability only after
   verified wins in two families.
4. **Proposal D**: `docs/invention-lab/proposals/iteration-algorithms.md`.
   Evaluate A (per-family μ+λ frontier, k 4–6), B (quality-diversity archive,
   family × strategy-behavior cells), C (islands with strategy migration: the
   transfer experiment), D (bandit allocation + successive halving), E
   (recursive: mutator prompt and feedback format archived and evolved) against
   the causes: wrong-family feedback, prescribed strategy in the prompt,
   population of one, 6 instances with margin 1, under-tested selection (from
   A), blind adapter features (finding 1). Build on #144 instead of
   re-specifying it: say what it already covers and what each option adds.
   Recommend A + D first, B as A's storage, C after A is stable, E only after A
   beats random search on two families. With the split, mutation can target
   strategy or adapter separately, and C migrates strategies only. Predeclare
   metrics: per-family frontier success over rounds; families where the frontier
   beats seeded random search; families where a strategy wins outside the one it
   was introduced in. No model calls, no synthetic curve presented as real.
5. **PRs**, each under 600 diff lines: (1) harness `labworld.py` + `replay.py`;
   (2) split + `test_split.py` + README; (3) spec B; (4) proposal D. Mark all
   as proposals; no completion-gate claims.

## 4. Hard limits (from the brief)

No forge, executor, kernel, console or bridge files without an accepted claim
on issue #1. No provider calls. Synthetic results never presented as real.
Every number in a doc cites a file on `main`. Eric's other lane (lab UI,
`apps/web/src/invention-lab/**`) is separate; do not mix it in.

## 5. Draft claim for issue #1

> Eric (research lane): claiming `experiments/strategy-adapter/**` and two new
> proposal docs, `docs/invention-lab/proposals/strategy-adapter.md` and
> `docs/invention-lab/proposals/iteration-algorithms.md`. No forge, executor,
> core, kernel or UI files. Branch `eric/strategy-adapter-split`: a hand split
> of parent `8215c367` into strategy/adapter/glue that replays the retained
> rough-transport run exactly (6 experiments, 5,205 steps, `2483f08f…`), with a
> stdlib replay harness. Builds on #144 and `claude/regression-analysis`, does
> not duplicate them. One note for the analysis author: rough-transport has
> `gap = step_height = 0`, so the parent's hardest-first order was a no-op
> there; its 6/6 came from its first admissible grid point.
