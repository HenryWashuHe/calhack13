# Combined integration checkpoint

Branch: `codex/combined-integration`, [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25).
This is an assembled development checkpoint
for both machines. It is draft because the source reviews below still have open
findings. Passing the existing suite does not resolve those findings.

The [PR review queue](PR_REVIEW_QUEUE.md) records the fresh review of all 18 open
PRs, dependency order, exact heads and source-fix ownership.

## Included snapshots

| Source | Exact head | Purpose |
| --- | --- | --- |
| Capability stack through #24 | `9e7939a` | Synthesis, bounded repair, durable acquisition/adoption, fresh verification, automatic two-organization scenario |
| #11 `feat/elastic` | `aa94d100` | Evidence projection, retrieval, Agent Builder specialists, Elastic capability search and jobs |
| #12 `feat/planner` | `22fefe62` | Mission planner and persisted decision/reconciliation API |
| #20 `feat/creao-readiness` | `e8cad205` | Asynchronous specialist readiness/jobs and CREAO environment registration |
| #21 `feat/ui-evidence` | `2c987193` | Capability evidence console |
| #22 `feat/elastic-planner` | `8a23273c` | Elastic Agent Builder model path for the planner |
| #23 `docs/claude-checkpoint` | `665aab42` | Claude's source handoff and development guide |

Merge resolutions preserve acquisition model/Apify configuration before Elastic
attachment, both Cargo dependency registrations, and every capability, adoption,
acquisition, finalization, specialist, outbox and projection worker. Source behavior
is preserved; these integration merges do not incorporate the separate failing
review probes or claim their defects are fixed.

## Validation

The assembled source baseline passes **666 Rust tests across 84 suites** with
`serde_json/preserve_order`, strict default-feature all-target Clippy and formatting.
The console passes **26 tests** and its TypeScript/Vite production build. The Rust-exported schemas and generated TypeScript types regenerate without drift.

The strengthened scenario fixture (`9d63cc0`) passes all five scoped scenario tests
and strict Clippy. A recording proxy proves B's actual automatic adoption sends
its committed requirement query through the real Rust Elastic client and receives
the repaired capability identity/source digest. The full native artifact pin is
checked against the canonical registry. Both organizations project/search their
own evidence, cross-mission searches return no foreign hits, and B retains that
configuration after restart. Fresh invocation proofs and both role handoffs remain
required. The full baseline ran before this test-only enhancement; the affected
five-test suite was then rerun successfully.

These are local fixtures: canned model/Apify responses, loopback Elastic APIs and
explicit development role bindings. CREAO's registered runtime still lacks a real
readiness verifier. Fetch's signed local recruitment proof remains separate from
kernel role activation. No live sponsor execution or cross-account Actor access
is established by this branch.

## Open source reviews

Keep fixes on the owned source branches, then merge their new commits into this
integration branch and rerun the affected checks. Avoid force-pushing the shared
checkpoint. The source PRs are not approved or merged into `main` by this branch.

| Source | Required follow-up | Evidence |
| --- | --- | --- |
| #11 | Mission/assignment expiration checks and managed-result finalization guard | [Review](https://github.com/HenryWashuHe/genesis/pull/11#pullrequestreview-5615583092); test-only `b2037e5`, five desired-safe failures |
| #12 | Durable requirement-create identity/uncertainty and status-aware rejected-change recovery | [Review](https://github.com/HenryWashuHe/genesis/pull/12#pullrequestreview-5614141053); test-only `70c4edb`, three desired-safe failures |
| #20 | Managed specialist result guard and authority check before each retry | [Review](https://github.com/HenryWashuHe/genesis/pull/20#pullrequestreview-5614181896); test-only `faf3d8b`, three desired-safe failures plus passing control |
| #21 | Bind displayed acceptance jobs to their installation and refresh active/failed run observations | [Source review](https://github.com/HenryWashuHe/genesis/pull/21#pullrequestreview-5615620684); no new browser reproduction |
| #22 / shared #11 client | Revalidate cached planner configuration and disable Agent Builder redirects | [Source review](https://github.com/HenryWashuHe/genesis/pull/22#pullrequestreview-5615620695); no new redirect/configuration-drift reproduction |

Claude retains planner, general lifecycle/saga/outbox, asynchronous role activation,
Elastic and UI ownership. Codex retains capability evolution/execution/verification
and this narrow integration checkpoint. No whole-kernel credit fallback is active.

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

Use a fresh local branch name if `local-combined` already exists. This checkout
contains no credentials; follow the individual provider setup guides. Run live
smoke checks only with the intended configured accounts and record their native
receipts separately from these fixture results.
