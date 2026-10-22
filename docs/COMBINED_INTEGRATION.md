# Combined integration checkpoint

Branch `codex/combined-integration`, [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25),
is pushed at `2829025802c405f01339d93956408f9e477babbb`. It is an assembled
development checkpoint, not a merge-ready release. Held source behavior remains
present even when its ordinary tests pass.

## Current scope

This checkpoint merges main through `09c13ee`, including the corrected specialist
execution #20, venue portal #35, planner #12, Elastic provider #11 and Elastic
planner #22. It preserves capability, adoption, acquisition, finalization,
specialist, outbox and projection workers and their startup configuration.
Only two documentation merge conflicts required resolution; production source
merged cleanly.

It still contains the earlier capability evidence UI #21, gateway readiness #26,
operator attach #27, Attention UI #28 and provider readiness probe #29 snapshots.
#27 includes sibling-agent binding fix #33; #28 includes the Abandon-only readiness
fix #31. Their individual findings were resolved, but the readiness dependency
still blocks this assembled branch.

Main subsequently gained capability gaps #49 and corrected evidence UI #21 at
`f8856f1`, then gap lifecycle UI #54 at `50db34b`. Those changes are not yet
in `2829025`. Reconcile them into the draft before relying on its console or gap
contracts. The newer #26 source/fix heads also need review before import; do not copy an unreviewed parent solely because
its PR has a new head.

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

## Remaining gate

The current [review queue](PR_REVIEW_QUEUE.md) holds readiness #26/#37 because a
mismatched early receipt can become Ready after a failed reconciliation write,
including after reopening the database. Fix it on the owned source branch,
rerun the unchanged negative probes, and then reconcile #26/#27/#28/#29. Passing
ordinary readiness tests does not resolve that demonstrated recovery path.

Source ownership stays scoped: Claude/Eric and Henry's Claude own their kernel,
planner, readiness and UI slices; Codex owns capability/provider implementation,
review and narrow integration. The combined draft must not bypass held source
reviews. No whole-kernel takeover is active.

All reported checks use local fixtures, canned provider responses or explicitly
labeled development bindings. A gateway callback establishes challenge
reachability, not installed skills or mission competence. Native sponsor runs,
CREAO core mission work and its genuine share link, a public portal, and permitted
cross-account Actor execution still require separate evidence.

## Reproduce

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
