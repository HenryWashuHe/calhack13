# Strategy / adapter split for method packages (proposal)

**Status: a proposal in Eric's research lane**
([claim](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6391308515)).
It is not a contract, changes no retained schema, and closes no gate. The implementation it
describes is the hand split in PR #181 (stacked on the replay harness, PR #180); neither is on
`main` yet. No model call, no new score.

## Why split a method at all

A generated method is one file that mixes two things: how it searches (which instance to try,
which candidate next, when to stop, what to submit) and what the task is (the line protocol, which
instance features matter, the design space, how a point becomes a design). Only the first can
plausibly move between task families. The first generated parent, `8215c367…`, shows why this
matters: it scored 1, 6 and 1 of 6 on ravine-crossing, rough-transport and return-mix
(`docs/invention-lab/evidence/live-method-20261024-a-return/README.md`, "Across the three families
so far"). From one file it is impossible to say which part carried the 6 and which part failed the
two 1s. A split makes those parts separately identifiable, separately mutable and separately
transferable.

## Interfaces, as implemented in #181

Three files in one ordinary package under `experiments/strategy-adapter/parent-split/`:

| File | Holds | Must not hold |
| --- | --- | --- |
| `strategy.py` | `order`, `propose`, `allocate`, `stop`, and `search` composing them | domain nouns, protocol I/O |
| `adapter.py` | line protocol, instance features, candidate space, design builder | search loops that issue requests |
| `method.py` | glue and entry point | logic |

**Strategy** (domain-free; it sees opaque items and candidates):

- `order(items, hardness)`: the order in which items are screened.
- `propose(axes, realize)`: candidates, as the grid points `realize` accepts.
- `allocate(results, n)`: the index of the next item for the current candidate, or `None`.
- `stop(wins, n)`: whether the whole search ends after this candidate.
- `search(items, candidates, evaluate, halted)`: composes the four and returns the candidate to
  submit, or `None`.
- `MAX_EVALUATIONS`: the method's own evaluation cap.

The only contract between the halves is the `evaluate` callback: `evaluate(item, candidate)` returns
`(passed, progress)` with a boolean and a number, or `None`, meaning the run cannot continue (cap
reached, grant exhausted, refusal or unknown outcome); `halted()` reports the same state.

**Adapter** (domain-specific; it does not search):

- `Session(limit)`: the executor's line protocol (`experiments/invention-lab/executor/PROTOCOL.md`),
  with `items()`, `evaluate(item, design)`, `halted()` and `submit(design)`.
- `hardness(item)`: the instance feature the strategy orders by.
- `budget(items)`, `AXES`, `realizer(cap)`, `build(...)` and `default()`: the candidate space and how
  a point of it becomes a design within the build budget.

**Glue**: `method.py` reads the items, orders them, proposes candidates, runs the search and submits.
It holds no logic of its own.

## Identity

- **The package identity scheme does not change; the package digest does.** The split is an
  ordinary `development-method-source/1` package: up to 256 files, sorted by path, one SHA-256 over
  the canonical artifact (`docs/invention-lab/contracts/packages.md`). Changed package bytes create
  a new whole-package digest and exact method reference. New trial plans, receipts and scoring bind
  that reference; retained records keep the original package reference unchanged.
- **Two derived identities travel beside it, as a sidecar.** `strategy` and `adapter` are each the
  SHA-256 of that file's bytes inside the package (and `glue` likewise). A sidecar names the package
  by its exact reference and lists `{ path, sha256 }` for each role. Anyone holding the package can
  recompute it, so it adds no trust and needs no signature. It sits outside the package: putting it
  inside would change the hash it describes.
- **Until the contracts owner decides**, the sidecar is advisory: nothing in the forge, executor or
  core reads it, and no retained record depends on it. A schema with named roles (for example a
  future `development-method-source/2`) is the contracts lane's decision, not this proposal's.

## What a mutator may change

| File | May change | Must keep |
| --- | --- | --- |
| `strategy.py` | the bodies of `order`, `propose`, `allocate`, `stop` and `search`; `MAX_EVALUATIONS`; new helper functions | the five signatures and the `evaluate` / `halted` contract; pure standard-library code with no I/O and no domain nouns |
| `adapter.py` | `hardness` and other instance features; `AXES`; `build`, `realizer`, `budget`, `default` | `Session` and `send` byte-identical (the protocol binding belongs to the executor's contract); no loop that issues requests |
| `method.py` | nothing | changed only by hand, with the replay check below |

**One target per candidate.** A candidate changes the strategy or the adapter, never both, so a
result can be attributed to one part. The untouched file's derived hash must equal its parent's;
the sidecar makes that a byte comparison before any trial. The static checks in #181's
`test_split.py` (no domain nouns in the strategy, no request-issuing loop in the adapter) are lexical
heuristics, not proofs; they catch accidents, not determined misuse.

## Acceptance rule: replay-identical, then live

Two kinds of change need different evidence.

1. **A change claimed to preserve behavior** (a split, a re-split, a refactor, a port): first
   **replay-identical**. `harness/replay.py transcript` serves every retained run of the original
   that is on `main` and must see the same requests in the same order and the same submission;
   `replay.py diff` must find no difference on synthetic oracle cases (synthetic outcomes are for
   equivalence only, never scores). Then **live**: `lab-exec run`, `lab-score score` and
   `lab-exec check` use a newly declared comparison plan with the split package's exact reference
   and unique run, attempt and scoring IDs. Keep the intended context, instances, grants and
   controls comparable, and compare corresponding requests and outcomes without rewriting the
   original records. A replay failure ends it; a live run is not a substitute, because it covers
   one comparison where replay covers every retained transcript.
2. **A mutation** (claimed to behave differently): replay does not apply. The predeclared trial
   decides, as for any candidate: a `development-trial-plan/1` with `success-count-margin`
   (`docs/invention-lab/contracts/trials.md`) against the controls, with seeded random search among
   them as in the retained `vs-random-search` trials, or the iteration contract proposed in #144
   once it is agreed.

The parent split passed both steps of rule 1 against the parent's retained rough-transport run:
6 experiments, 5,205 steps, the same design `2483f08f…` every time, 6 of 6
(`docs/invention-lab/evidence/live-method-20261024-b-revision-receiver/vs-parent/control/transcript.json`,
`…/vs-parent/verdict.json`). #181 reports both checks as an author-run local process, in
`namespaces` mode for the live run; they are not retained or independent evidence.

## Finding 1: the difficulty order was a no-op on rough-transport

The parent's "screen on the hardest instance" sorts by `gap + step_height` (parent `method.py`
lines 64 to 67, in `docs/invention-lab/evidence/live-method-20261024-generated/method-package.json`).
Those are ravine fields: the generator leaves both at 0.0 for rough-transport
(`experiments/invention-lab/evaluator/src/family.rs`, lines 110 to 111 and 137 to 151). On that family
the sort was a no-op. In the particular original six-experiment rough-transport trace cited above,
the parent screened instance 0 because it came first, and its 6/6 came from the first admissible
grid point. This does not describe every later rough-transport trace. The merged regression
analysis already states this correction
(`docs/invention-lab/analysis/why-generation-2-regressed.md`, §1, PR #160).

What the split adds is where the fault sits. The screen-then-confirm loop is in `strategy.py`
and behaved as intended. The blind feature is `adapter.hardness`, the part that cannot transfer.
In one file the two were tangled, so feedback about "the hardest-first strategy" would have
blamed or credited the wrong part. A cheap guard follows: before a run, flag any adapter feature
that is constant across the family's public development instances, because a strategy ordering by
it is ordering by nothing. That flag is a lint for a mutator's input, not a score.

## When a strategy may be called a capability

A strategy is called **a strategy** until it has **verified wins in two families**:

- the same strategy bytes (identical derived `strategy` hash), each time paired with that family's
  own adapter;
- in each of two distinct families, an `accepted` verdict from a predeclared trial against that
  family's seeded random search and its current incumbent;
- the quoted numbers are those of a final fresh panel never used for selection (the confirmation
  step proposed in #144; `HiddenCommitment` in `docs/invention-lab/contracts/trials.md`).

Wins by different strategy bytes do not add up. "Capability" here is claim language only; adopting
anything into an organization still goes through the receiver's own acceptance and adoption path.
Today no split strategy qualifies under that two-family rule. [PR #165](https://github.com/HenryWashuHe/genesis-history-experiment/pull/165)
records author-run acceptance of the unchanged parent on rough-transport against seeded random
search: public 12/12 versus 7/12, and hidden attempt 2, 12/12 versus 9/12. That single-family parent
result does not establish a verified two-family split strategy or recursive improvement.

## Open questions for the lane owners

- **Contracts (Codex):** keep the role hashes as a sidecar, or name roles in a future package
  schema?
- **Forge / generation:** should a generation request carry `target: strategy | adapter`, with the
  other file frozen and checked by hash?
- **Executor and scoring (Henry):** nothing is required; this proposal needs no executor or scorer
  change.

## Limits

One method, split by hand, with author-reported replay and live checks on one family. The split is
not generated; those checks do not independently establish general equivalence or improvement.
Whether mutating the halves separately produces better methods is an open empirical question for
the iteration proposal (`iteration-algorithms.md`).
