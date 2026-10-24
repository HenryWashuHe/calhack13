# Combined integration checkpoint

[PR #25](https://github.com/HenryWashuHe/genesis/pull/25) is closed without merging.
Its branch `codex/combined-integration` remains at
`2829025802c405f01339d93956408f9e477babbb` for historical reference. All 17
combined-only merge commits and remaining unique content were audited; no pending
runtime was absent from the reviewed main path.
Its useful Elastic recording-proxy scenario was preserved in
[#68](https://github.com/HenryWashuHe/genesis/pull/68), merged at `2adaff4`.
Use current main, not this historical assembled checkpoint, for ongoing work.

## Scope of the historical draft

This checkpoint merges main through `09c13ee`, including the corrected specialist
execution #20, venue portal #35, planner #12, Elastic provider #11 and Elastic
planner #22. It preserves capability, adoption, acquisition, finalization,
specialist, outbox and projection workers and their startup configuration.
Only two documentation merge conflicts required resolution; production source
merged cleanly.

It still contains the earlier capability evidence UI #21, gateway readiness #26,
operator attach #27, Attention UI #28 and provider readiness probe #29 snapshots.
#27 includes sibling-agent binding fix #33; #28 includes the Abandon-only readiness
fix #31. These snapshots predate the final reviewed main integrations; passing this
draft's tests does not substitute for the later fixes.

Main subsequently gained capability gaps #49 and corrected evidence UI #21 at
`f8856f1`, then gap lifecycle UI #54 at `50db34b` and the offline gap-policy
materializer #56 at `00ad205`, gap event log #61, evaluation #52 and planner
lifecycle/gap wakeup #63/#57. Main then gained portal holdout #60, corrected
gateway readiness #26/#62, operator attach #27, Attention UI #28, durable gap driver
#66 and CREAO readiness probe #29. Those changes
are not in `2829025`. Use the current [review queue](PR_REVIEW_QUEUE.md) for
source-specific integration evidence before choosing a demonstration checkout.

## Validation at this checkpoint

132 affected Rust tests passed with `serde_json/preserve_order`: 57 planner/Elastic
and 75 server checks across 26 nonempty suites. Formatting, whitespace and strict
default-feature workspace/all-target Clippy passed. Web/schema sources are
unchanged from prior draft `f5e747c`, so those checks were not repeated. This is
focused integration evidence, not a new full-workspace total.

The historical full run at `1004934` passed 699 tests. Earlier web evidence at
`8014683` passed 37 tests plus build. Neither total describes a new full run of
`2829025` or current main. Detailed historical scope is retained in
[the prior checkpoint](https://github.com/HenryWashuHe/genesis/blob/a857a9f/docs/COMBINED_INTEGRATION.md).

## Reuse and current status

The original readiness recovery and migration findings are resolved on main.
#26 merged at `aadb15d`, #27 at `574c5b8` and #28 at `0b91ddf`; the last tree
matches tested composition `0862df8`. Main also contains #66 at `e2563ef` and the
final #29 composition at `ba4c920`. These results do not update the old draft's
source or validate its earlier probe snapshot.

The #68 extraction keeps the scenario from `9d63cc0` without unique runtime edits.
Its targeted repaired A→B test passed in 97.72 seconds with real Rust Elastic
retrieval and projection against loopback fixtures, organization-isolated evidence
and restart checks; final composition Clippy passed. This focused result is not a
rerun of the historical 699 tests. Current source-specific counts remain in the
[review queue](PR_REVIEW_QUEUE.md).

Source ownership stays scoped: Claude/Eric and Henry's Claude own their kernel,
planner, readiness and UI slices; Codex owns capability/provider implementation,
review and narrow integration. The preserved branch is historical; new work belongs on reviewed main.
No whole-kernel takeover is active.

All reported checks use local fixtures, canned provider responses or explicitly
labeled development bindings. A gateway callback establishes challenge
reachability, not installed skills or mission competence. Native sponsor runs,
CREAO core mission work and its genuine share link, a public portal, and permitted
cross-account Actor execution still require separate evidence.

## Reproduce the historical checkpoint

```sh
git fetch origin
git switch --create local-combined --track origin/codex/combined-integration
cargo test --locked --workspace --features serde_json/preserve_order
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p genesis-server -- export-schemas
cd apps/web
npm ci
npm test
npm run build
npm run gen:types
```

Use a fresh local branch name if `local-combined` already exists. Keep build caches
separate between worktrees. Provider setup and live receipts belong to their
individual guides; this checkout contains no account credentials.
