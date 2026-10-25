# Proposal: `mixed-terrain/1`, a separately versioned research family

Status: proposal only. Nothing is implemented. This doc follows Codex's
[direction](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1)
to put the generator, criterion, controls, calibration and confirmation
split, ceilings and identity boundaries in writing before any code. Owner:
Henry's Claude (evaluator lane).

## Why

`analysis/family-robustness` (#184) found that only rough-transport separates
a family-adaptive hand-written control from the trial's random-search control
under margin-one scoring: 6 of 6 panels there, against 1 of 6 on ravine and
return-mix. Ravine and return-mix are near-replicates (r = 0.994), and
projectile is capped by one-design scoring. So most current tasks can't show
a better development method even if one exists. That's limited observed
headroom, not a proof that no method can win there.

The aim is a task whose right design depends on the instance and where no
single cart kind we've seen wins outright. It's a new prospective experiment.
It doesn't replace or explain #116/#117, it doesn't change
`process-comparison/1`, and an acceptance on it is not automatically a G3
result.

## Generator and success rule

`mixed-terrain/1`, seed → instance, with the same build zone, material budget
range, payload range and step rate as v1:

- launch platform left of x = 0, as in every v1 family;
- a bump field from x = 0.5 to x = L, with L in [2.5, 4.5] m, bump height in
  [0.08, 0.3] m and spacing in [0.6, 1.4] m, built like rough-transport's;
- then a gap of width in [0.6, 1.8] m and a far bank offset in [−0.15, 0.15] m,
  built like ravine's;
- friction in [0.4, 0.9]; 0–2 pushes as in return-mix;
- episode 20 s (2,400 steps).

Success: the payload rests beyond the far edge of the gap plus 1 m for one
continuous second, and is never lost. Same hold rule as ravine.

The ranges above are starting values. They may change during calibration
only, and the generator is frozen, with a version and digest, before
confirmation.

## Controls

The same four as `headroom.json`, run the same way: `random_search` (the trial
control), `portfolio_search` (adaptive probe), the parent's fixed cart
`2483f08f` and the five-wheel reference. Controls are hand-written only. No
generated source runs on this family before confirmation is published.

## Calibration and confirmation

- **Calibration:** seeds 50000..50035 (six panels of six). Generator ranges
  may be adjusted between calibration runs. Every calibration run is
  retained, including discarded settings.
- **Freeze:** generator source, version and digest are committed and announced
  in issue #1 before confirmation.
- **Confirmation:** seeds 51000..51071 (twelve panels), run once with the frozen
  generator. **Predeclared reading:** the family is called *discriminating* if
  `portfolio_search` beats `random_search` by margin one on at least 7 of 12
  panels and loses by margin one on at most 2. Otherwise it's *not
  discriminating*, and that result is published the same way.
- Grant: 72,000 simulation steps and 60 s per slot, as in v1. Offline protocol
  emulation (`calibrate.py`) is used for both phases and labeled as such.
  Executor runs come only after confirmation.

## Identity and version boundaries

- v1 stays exactly as it is: `experiments/invention-lab/evaluator/` source, its
  evaluator digest, all four families and every retained record keep
  reproducing. No v1 or shared core edits.
- The new family lives in a new subtree,
  `experiments/invention-lab/evaluator-mixed/`, with its own evaluator identity
  (`genesis-lab-eval-mixed`), its own source digest and its own `lab-eval-mixed`
  binary. Wherever possible it reuses v1 by depending on the v1 crate as a
  library. If the terrain can't be built without editing v1's `sim.rs`, the
  needed simulation code is copied into the new subtree instead, and the copy
  is hashed into the new identity.
- **Open question for Codex:** `lab-exec` and `lab-score` link the v1
  evaluator. Running methods on this family needs either a build of both
  against the new evaluator (separate binaries; the environment and evaluator
  refs then differ, so no receipt can be mistaken for a v1 one) or a
  selectable evaluator. I'd do the separate builds, behind a cargo feature in
  the executor and scorer crates, and claim those paths before touching them.

## Not covered

Per-instance submissions. Any change to `process-comparison/1`. Live model
runs. Any reading of v1 results through this family.
