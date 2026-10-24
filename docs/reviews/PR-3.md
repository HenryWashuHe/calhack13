# PR #3 review — kernel and console

Reviewed head `21103d1` against main, October 21, 2026. These findings were
reproduced against the actual implementation, not inferred from test coverage.
Claude owns the fixes below; Codex owns the separate capability integration path
in [the next implementation split](../NEXT_IMPLEMENTATION_SPLIT.md).

## Fix before relying on lifecycle and recovery guarantees

1. **P1: revoked executors can still commit.** `genesis-core/src/engine.rs:781–788`
   replaces a delegated executor without advancing the responsibility epoch;
   `985–989` clears a retired executor the same way. Old grants still pass
   `check_result_epoch`, the current-state check used by job result submission.
   Fence superseded assignments and validate the job's executor/assignment, not
   just an unchanged responsibility epoch. Test delegation replacement and
   retirement with a delayed result and assert no ledger/responsibility effect.
2. **P1: budget overflow bypasses authority.** `engine.rs:535` and
   `model.rs:302–303` add/sum unsigned budgets without overflow checks. Roles
   with budgets `u64::MAX` and 1 bypass a zero budget in release and panic in
   debug. Reproduced for bootstrap and an agent-submittable change. Use checked
   accumulation and return a validation error; test both paths in release too.
3. **P1: completed responsibilities still accept old work.**
   `engine.rs:1555–1572` accepts the old epoch after completion releases its
   assignment. A second in-flight job can still reach the successful result path.
   Require a live responsibility/assignment and appropriate executor before any
   ledger or completion effect.
4. **P1: dispatch can permanently lose delivery.**
   `genesis-server/src/routes/jobs.rs:106–113` inserts the job/idempotency key
   before the separate event/outbox transaction. An injected SQLite fault leaves
   a durable queued job without delivery; same-key retries return 200 and never
   enqueue it. Persist job, request identity, event and outbox atomically.
5. **P1: submission can strand a proposal.** `genesis-server/src/saga.rs:51–54`
   inserts `PROPOSED` before the validation/event transaction at `79–83`.
   A fault leaves a valid proposal outside `resume_pending`'s states, while
   retries return it unchanged. Make initial insertion/status/events atomic or
   explicitly and safely resume proposals.

## Provider recovery and evidence/UI correctness

6. **P2: readiness loses known resource identity.** `saga.rs:160–181` checks
   readiness before persisting the binding. A readiness failure leaves a terminal
   failed record and events without the already-created agent ID. Persist the
   resource identity before readiness so reconciliation/cleanup can find it.
7. **P2: readiness has no deadline.** `saga.rs:156–160` bounds `provision` only.
   A hanging `check_ready` exceeds the configured timeout indefinitely. Apply the
   deadline to the complete provisioning/readiness sequence and preserve identity.
8. **P2: SSE catch-up loses persisted events.** `routes/events.rs:85–92` loads
   just one 5,000-event page after lag. An actual router/SQLite probe paused after
   seq 1 and appended 7,000 events: output reached 5,001 then jumped to 5,978,
   skipping 976 persisted events. Drain all pages or backfill every live sequence
   gap before advancing the cursor; test more than one page.
9. **P2: overlapping snapshot loads regress the UI.**
   `apps/web/src/lib/useOrg.ts:60–73` accepts an older request completing after a
   newer one. The actual hook went from revision 2 back to 1. Use request
   generations/cancellation and avoid accepting older revisions for the same org.
10. **P2: reconnect does not repair failed snapshot refresh.** `useOrg.ts:95`
    only sets connection status on open. Following a failed REST refresh and a
    quiet reconnect, stale/null state remains while the UI says live. Reload on
    successful connection and clear/retry the snapshot error appropriately.
11. **P2: historical graph can be mislabeled.** `OrgPanel.tsx:140–149` keeps
    revision 1's historical snapshot after selecting revision 2 and failing that
    request, while the banner says revision 2. Clear or key data to the requested
    revision and render an explicit failure/loading state.
12. **P2: requirement and responsibility ownership disagree after handoff.**
    `engine.rs:717` updates responsibility ownership but leaves the linked
    requirement's `owner` unchanged. Update both within the same transition and
    assert that the UI/domain has one accountable owner after handoff.
13. **P2: bootstrap permits reporting cycles.** `engine.rs:1232–1245` accepts
    self/two-role cycles that `ChangeReportingEdge` rejects. Apply the same
    reporting graph invariants during bootstrap.

## Reproduction artifacts

`pr-3-probes/review_engine.rs.txt` and `review_durability.rs.txt` contain the
temporary probes used for review. Copy them into the corresponding crate's
`tests/` directory to reproduce, then remove the copied probes. They intentionally
assert current faulty behavior and are **not regression tests to keep unchanged**.
Convert their assertions to the intended behavior when implementing fixes.

```sh
cargo test --locked -p genesis-core --test review_engine
cargo test --locked --release -p genesis-core --test review_engine
cargo test --locked -p genesis-server --test review_durability
```

Seven core probes and four durability probes reproduced successfully; existing
seven web tests and the web production build passed despite the UI defects.
The optional JSON key-order/configuration-digest findings from the previous
[integration checkpoint](../KERNEL_CAPABILITY_HANDOFF.md) also remain relevant.

Codex is addressing the incomplete capability executor and report trust boundary
in its own integration branch. A dev stub's passed flag is not evidence of live
verification, and no review finding here claims that live sponsor integration has
already happened.
