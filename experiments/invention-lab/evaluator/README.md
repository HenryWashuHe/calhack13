# Invention-lab evaluator (experimental)

Henry's Claude, genesis#126. Implements the contract in
[docs/invention-lab/evaluator.md](../../../docs/invention-lab/evaluator.md):
design language v1, a deterministic 2D rigid-body simulation (Rapier 2D 0.22
with `enhanced-determinism`), four task families, metered development
experiments, acceptance and comparison scoring, and commit/reveal hidden
instances. **Simulated results say nothing about physical machines.**

Standalone crate (its own `[workspace]`), not a member of the root workspace
until the #18/#22 interfaces are agreed. No provider, kernel, API or UI code.

```sh
cd experiments/invention-lab/evaluator
cargo test --release
cargo run --release --bin lab-eval -- instance ravine-crossing 3
cargo run --release --bin lab-eval -- accept designs/reference-five-wheel.json ravine-crossing 0 1 2 3
cargo run --release --bin lab-eval -- experiment designs/cart-long.json ravine-crossing 0 100000
cargo run --release --bin lab-eval -- commit cmp-1 my-salt return-mix 5
cargo run --release --bin lab-eval -- compare designs/reference-five-wheel.json cmp-1 return-mix 5 <commitment> my-salt 0
```

| Family | Task | Notes |
| --- | --- | --- |
| `ravine-crossing` | carry the payload across a 1.0–2.5 m gap and hold it on the far bank for 1 s | A's visible task |
| `rough-transport` | carry it 8 m over bumps (0.10–0.35 m) through 1–4 pushes without it touching the ground | B's related task |
| `return-mix` | ravine plus pushes and lower friction | A's return challenge; use hidden instances |
| `projectile-target` | land the payload in a 1 m zone 4–9 m away; a wall at x = 1.5 m stops the launcher | non-reuse control |

Outcomes follow #22: `scored` (with `success` as a metric), `invalid` (design
validation), `failed` (simulation diverged), `unknown` (worker stopped).

Calibration notes, local and small. `designs/reference-five-wheel.json` is a
hand-built sanity design, never a method or a baseline. On development seeds
0–19 it crosses 13/20 ravines and passes 8/20 rough tracks;
`designs/cart-long.json` crosses 0/20 ravines and passes 10/20 rough tracks.
A crude random search over catapult parameters found designs that land 2 of
10 projectile instances. The same trace digests come out of debug and release
builds on x86_64; other architectures haven't been checked.
