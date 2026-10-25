# Iteration engine (proposed, exploratory reference)

Executable spec for `docs/invention-lab/contracts/iteration.md`, a proposal under interface
review: a domain-free loop that improves a method across generations without touching model
weights, and passes the result on with its evidence. Node only, no dependencies, no provider
calls. Not a second orchestrator; its ledger and score objects do not replace the retained
contracts (method packages, trial plans, execution and scoring receipts, chain cost records).

- `engine.mjs`: `createState`, `runGeneration`, `run`, `confirm` (final fresh panel, never used
  for selection), `exportBundle`, `adopt`, `verifyLedger`.
- `synthetic-family.mjs`: a SYNTHETIC family, controls and mutator used only by the tests and the
  demo below. It proves the protocol, not any task.
- `engine.test.mjs`: eight probes (ordering, promotion margin, rejection, ledger tamper detection,
  determinism, monotone frontier, stop rules, inheritance with re-verification).
- `run-synthetic.mjs`: prints a ledger summary for a few generations and one adoption.

```sh
node --test scripts/iteration/engine.test.mjs
node scripts/iteration/run-synthetic.mjs
```

Honesty: nothing here has run on a real task family. The binding to `lab-exec`, `lab-score` and
the generation step is described in the contract's last section and is not implemented.
