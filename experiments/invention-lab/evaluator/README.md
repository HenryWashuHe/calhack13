# Invention-lab evaluator (experimental)

Henry's Claude, genesis#126. Implements the contract in
[docs/invention-lab/evaluator.md](../../../docs/invention-lab/evaluator.md):
design language v1, a deterministic 2D rigid-body simulation (Rapier 2D 0.22
with `enhanced-determinism`), four task families, and commit/reveal hidden
instances. **Simulated results say nothing about physical machines.**

These are **simulation primitives**, not the enforced contract:

- No grant or ledger lives here. `experiment` takes a reservation the caller's
  ledger already made and reports measured steps (`None` = unknown, never 0).
  Cumulative charging is the trusted experiment ledger (#32) in the executor.
- `score_acceptance` / `score_comparison` simulate; the scoring grant, comparison
  closure and #22 request/execution/output binding belong to a separate
  trusted scoring adapter that doesn't exist yet.
- `evaluator_digest()` hashes the scoring sources, manifest and lockfile
  (`score::digest_files`), so a source edit changes it under the same version.
  It identifies the evaluator; it doesn't authenticate a binary or cover the
  compiler and platform.
- Hidden sets use the immutable `genesis-lab-hidden/2` descriptor with
  length-prefixed fields; the salt is checked against it, never added to it.

Standalone crate (its own `[workspace]`), not a member of the root workspace
until the #18/#22 interfaces are agreed. No provider, kernel, API or UI code.

```sh
cd experiments/invention-lab/evaluator
cargo test --release
cargo run --release --bin lab-eval -- instance ravine-crossing 3
cargo run --release --bin lab-eval -- accept designs/reference-five-wheel.json ravine-crossing 0 1 2 3
cargo run --release --bin lab-eval -- experiment designs/cart-long.json ravine-crossing 0 2400
cargo run --release --bin lab-eval -- commit cmp-1 my-salt return-mix 5 > descriptor.json
cargo run --release --bin lab-eval -- compare designs/reference-five-wheel.json descriptor.json my-salt 0
```

| Family | Task | Notes |
| --- | --- | --- |
| `ravine-crossing` | carry the payload across a 1.0–2.5 m gap and hold it on the far bank for 1 s | A's visible task |
| `rough-transport` | carry it 8 m over bumps (0.10–0.35 m) through 1–4 pushes without it touching the ground | B's related task |
| `return-mix` | ravine plus pushes and lower friction | A's return challenge; use hidden instances |
| `projectile-target` | land the payload in a 1 m zone 4–9 m away; a wall at x = 1.5 m stops the launcher | different-task control; transfer to it is measured, not assumed |

Outcomes follow #22: `scored` (with `success` as a metric), `invalid` (design
validation), `failed` (simulation diverged), `unknown` (worker stopped).

Calibration notes, local and small, with the 2000 N·m motor torque ceiling.
`designs/reference-five-wheel.json` is a hand-built sanity design, never a
method or a baseline. On development seeds 0–19 it crosses 13/20 ravines and
passes 3/20 rough tracks; `designs/cart-long.json` crosses 0/20 ravines and
passes 8/20 rough tracks. The projectile family hasn't been recalibrated since
the torque ceiling. The same trace digests come out of debug and release builds
on x86_64; other architectures haven't been checked.
