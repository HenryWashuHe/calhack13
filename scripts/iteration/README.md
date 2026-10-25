# Iteration engine (proposed, exploratory reference)

Executable spec for `docs/invention-lab/contracts/iteration.md`, a proposal under interface
review: a domain-free loop that improves a method across generations without touching model
weights, and passes the result on with its evidence. Node only, no dependencies, no provider
calls. Not a second orchestrator; its ledger and score objects do not replace the retained
contracts (method packages, trial plans, execution and scoring receipts, chain cost records).

- `emit.mjs`: `toTrialPlans`, `toCriterion`, `toVerdicts`: one acceptance `development-trial-plan/1`
  per (candidate, control-arm) pairing with exactly one candidate and one control slot and
  trial-qualified identities; the adapter supplies package references and dependencies, raw source
  hashes travel separately under `iteration-source/1`; verdicts are per pairing with `candidate`,
  `control`, `of` and `insufficient`, independent of the engine's one-frontier selection.
- `engine.mjs`: `createState`, `runGeneration`, `run`, `confirm` (final fresh panel, never used
  for selection), `exportBundle`, `adopt`, `verifyLedger`.
- `synthetic-family.mjs`: a SYNTHETIC family, controls and mutator used only by the tests and the
  demo below. It proves the protocol, not any task.
- `engine.test.mjs`: nine author probes; `emit.test.mjs`: six emitter probes.
- `review-budget.test.mjs`, `review-provenance.test.mjs`, `review-resume-transfer.test.mjs`,
  `review-partial-ancestor.test.mjs` (Codex) and `review-iteration.test.mjs`,
  `review-henry-probes.test.mjs`, `review-henry-probes2.test.mjs` (Henry's Claude): the reviewers'
  42 adversarial probes, retained byte-identical and passing.
- `run-synthetic.mjs`: prints a ledger summary for a few generations and one adoption.

```sh
node --test scripts/iteration/engine.test.mjs
node scripts/iteration/run-synthetic.mjs
```

Honesty: nothing here has run on a real task family. The binding to `lab-exec`, `lab-score` and
the generation step is described in the contract's last section and is not implemented.

## From protocol to optimizer (2026-10-25)

The owner's direction on October 25: upgrade the loop into an algorithm first, use the sponsors
meaningfully, accept a small honest smoke test over no result. These modules sit beside the engine;
the engine itself is unchanged except for the root-hydration fix.

- `families/jobshop.mjs`: the first REAL task family. 10 jobs × 5 machines, every job visits every
  machine once, processing times 1–99, instances derived from their seeded id. A package is one JavaScript
  dispatching rule `(op, ctx) => number` run in a `node:vm` sandbox with a timeout; a greedy dispatcher
  starts the highest-priority ready operation. Success is makespan ≤ `target` × lower bound (max machine
  load, max job length); progress is lower_bound / makespan. Calibration on 20 fresh instances put
  `target = 1.20` mid-band (SPT 0.40, MWKR 0.50, LPT 0.10, random-search-of-24 0.65 success fraction), so
  that is the default. Controls: `fixed-spt`, `fixed-mwkr`, `fixed-lpt`, and `random-search`, the best
  of N seeded linear rules chosen on a private training panel with the loop's own selection budget
  (`evidence` records N and the panel). Residual sandbox limit: sources naming `import`/`async`/`await`/
  `Promise` are refused because a sandbox rejection cannot be intercepted; an obfuscated source could
  still build one. Acceptable for model-proposed rules; not a security boundary.
- `families/binpack.mjs`: the second family, online bin packing. 100 items of size 1–50 into capacity-100
  bins; a package is a bin-choice rule `(item, bin, ctx) => number` over the open bins that fit; success is
  bins ≤ 1.02 × ceil(total / capacity), which at this size equals hitting the volume bound (calibration on 20
  fresh instances: first-fit 0.35, best-fit 0.45, worst-fit 0.00, random-search-of-24 0.40). Controls:
  `fixed-first-fit`, `fixed-best-fit`, `fixed-worst-fit`, `random-search`. Known property: with sizes in
  [1, 50] first-fit and best-fit tie on most instances, so this family separates rules less sharply than
  job-shop; it is here to show the same engine, mutator, cascade and exports run unchanged on a second domain.
- `families/sandbox.mjs` + `families/rule-child.mjs`: where generated rules run. `node:vm` is not a security
  boundary (a rule's top-level `this` reaches the host realm), so every rule, controls included, is scored in
  a child process started with an empty environment, an empty working directory, a wall-clock timeout and
  SIGKILL; the parent never evaluates model text, and the API key never enters the scoring process. Residual:
  CPU until the wall timeout, and grandchildren that outlive the child's kill; `lab-exec`'s namespace mode is
  the stronger boundary. Thrown values are reported through a bounded primitive diagnostic that invokes no
  getters. Run directories are evidence: an occupied `--out` is refused before any write.
- `mutators/claude.mjs`: the model mutator. One Messages API request per generation through a child
  process (`claude-call.mjs`, raw HTTP, key from `GENESIS_LLM_API_KEY`, server-side `fallbacks: "default"`)
  so the engine stays synchronous. The prompt follows the FunSearch/AlphaEvolve finding: scored programs,
  worst first and best last, with id-free per-instance tables, plus a reflection record of the last
  parent-versus-best-child pair (ReEvo). No strategy is prescribed. Failures and refusals return no
  candidates and are counted; nothing is thrown into the engine.
- `cascade.mjs`: asks for 2k proposals and screens them on a separate seeded screening panel before the
  sealed plan exists; keeps the top k. Screening evaluations are reported but are not selection evidence.
- `sponsors/elastic.mjs`: the verified ledger as one idempotent bulk body (`_id` = record hash); posts
  only when `ELASTICSEARCH_URL` and `ELASTIC_DATA_API_KEY` are set. With the current data key the post is
  accepted and every document is refused (`indices:admin/auto_create` unauthorized), which the result
  records verbatim; the index needs a key with create-index rights.
- `sponsors/fetch-offer.mjs`: an exported bundle as a `genesis-method-offer/1` chat message and as the
  observation body the ACP mission bridge already posts (`source`, `summary`, `evidence_ref`). The bridge
  does not yet send or parse it; the receiver re-verifies with `adopt` either way.
- `run-jobshop.mjs`: the runner for both families (`--family jobshop|binpack`; the name predates the second
  family). `--mock` uses a seeded local stand-in; without it, live model calls.
  Writes `result.json`, `ledger.json`, `bundle.json`, `elastic-bulk.ndjson`, `fetch-offer.txt`,
  `fetch-observation.json` under `results/`. After the loop it confirms the frontier on a fresh panel and,
  report-only, scores the root and every control on that same panel.

```sh
node --test scripts/iteration/*.test.mjs scripts/iteration/families/*.test.mjs scripts/iteration/mutators/*.test.mjs scripts/iteration/sponsors/*.test.mjs
node scripts/iteration/run-jobshop.mjs --mock --generations 3 --k 3 --heldout 8
node scripts/iteration/run-jobshop.mjs --generations 5 --k 4 --heldout 20   # live, needs GENESIS_LLM_API_KEY
node scripts/iteration/run-jobshop.mjs --family binpack --mock --generations 2
```

Honesty: a smoke test of this size establishes whether the loop can beat seeded random search on one
family on a fresh panel. It does not establish recursive improvement, transfer between real
organizations, or anything about other families. Results, positive or null, are written to
`docs/invention-lab/analysis/` with the run's files.
