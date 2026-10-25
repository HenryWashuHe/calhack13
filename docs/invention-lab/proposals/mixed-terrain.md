# Proposal: `mixed-terrain/1`, a separately versioned research family

Status: proposal only. Nothing is implemented. This doc follows Codex's
[direction](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1)
to put the generator, criterion, controls, calibration and confirmation
split, ceilings and identity boundaries in writing before any code. Owner:
Henry's Claude (evaluator lane).

## Why

`analysis/family-robustness` (#184, author-run, finite samples) observed that
under margin-one scoring the hand-written portfolio probe beat the trial's
random-search control on 6 of 6 rough-transport panels, but on only 1 of 6
ravine and 1 of 6 return-mix panels, losing two return-mix panels. Across
the 15 retained designs, ravine and return-mix success counts correlated at
r = 0.994, and a 12,000-evaluation random sample of launchers reached a best
observed 2 of 6 per projectile panel. These are observations about those
designs, controls and samples. They do not establish that two families are
equivalent, that projectile has a hard ceiling, or that the current tasks
cannot show a better development method. They do suggest that the observed
headroom for the controls we have is small outside rough-transport, which is
reason enough to add a prospective family and measure.

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

- **Prerequisite: a corrected, reviewed measurement harness.** The #184
  review found that `calibrate.py` coerces a failed or absent evaluator
  receipt to zero steps and `ok: true`, mixes unavailable acceptance with
  measured failure, has no end-to-end deadline on its blocking protocol
  read, and does not guarantee child cleanup. Those findings must be fixed
  and independently reviewed before any calibration or confirmation run on
  this family. Until then the harness cannot substantiate the grants or
  readings below. A failed, timed-out or unknown-usage panel is retained as
  **unavailable**, never as zero successes or zero cost.
- **Calibration:** seeds 50000..50035 (six panels of six). Generator ranges
  may be adjusted between calibration runs. Every calibration run is
  retained, including discarded settings and unavailable panels.
- **Freeze:** before confirmation, one commit pins and issue #1 announces the
  whole comparison, not just the generator: generator source, version and
  digest; each control's source bytes and, for fixed designs, the design
  identity; the harness script identities; the evaluator and `lab-eval-mixed`
  build identities; the scoring rule and margin; the panel-to-seed assignment;
  and the step and wall grants. Anything not pinned at freeze is not part of
  the confirmation.
- **Confirmation:** seeds 51000..51071 (twelve panels), run once with the frozen
  comparison. **Predeclared reading:** the family is called *discriminating*
  if `portfolio_search` beats `random_search` by margin one on at least 7 of
  12 panels and loses by margin one on at most 2, with every one of the 12
  panels available for both arms. If any panel is unavailable for either arm,
  the reading is *incomplete* and is published as such with the unavailable
  panels listed; it is not rerun. Otherwise the result is *not
  discriminating*, published the same way.
- Grant: 72,000 simulation steps and 60 s per slot, as in v1. Offline protocol
  emulation (the corrected `calibrate.py`) is used for both phases and
  labeled as such. Executor runs come only after confirmation.

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
