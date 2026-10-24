# Combined integration checkpoint

Branch: `codex/combined-integration`, [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25).
This is an assembled development checkpoint
for both machines. It is draft because the source reviews below still have open
findings. Passing the existing suite does not resolve those findings.

The [PR review queue](PR_REVIEW_QUEUE.md) distinguishes merged additions from
held source changes. Latest source integration `3961ecc` includes main `f03ce9b`
(CREAO execution receipts #32), #28 at `17b6b02` (UI fix #31), and #27 at
`5952987` (sibling-agent fix #33). All merged cleanly. #27/#28's own findings
are resolved; #26/#20 backend dependencies remain held. Henry's Claude has claimed the scoped #27/#28 corrections in issue #4;
other source ownership is unchanged.

## Included snapshots

| Source | Exact head | Purpose |
| --- | --- | --- |
| Main capability stack through #24 | `9e7939a` | Synthesis, bounded repair, durable acquisition/adoption, fresh verification, automatic two-organization scenario |
| Main receipt addition #32 | `050b099` (merge `f03ce9b`) | Bound, read-only CREAO native execution receipts; no domain-success claim |
| #11 `feat/elastic` | `aa94d100` | Evidence projection, retrieval, Agent Builder specialists, Elastic capability search and jobs |
| #12 `feat/planner` | `22fefe62` | Mission planner and persisted decision/reconciliation API |
| #20 `feat/creao-readiness` | `e8cad205` | Asynchronous specialist readiness/jobs and CREAO environment registration |
| #21 `feat/ui-evidence` | `2c987193` | Capability evidence console |
| #22 `feat/elastic-planner` | `8a23273c` | Elastic Agent Builder model path for the planner |
| #23 `docs/claude-checkpoint` | `665aab42` | Claude's source handoff and development guide |
| #26 `feat/creao-gateway-readiness` | `3dc95d6b` | Kernel gateway readiness challenges and receipts |
| #27 `feat/creao-attach` | `5952987` | Operator attachment, including reviewed sibling-agent collision fix #33 |
| #28 `feat/attention-ui` | `17b6b02` | Attention panel, failure drills and reviewed Abandon-only readiness fix #31 |
| #29 `codex/creao-readiness-probe` | `c1e6ba4` | Durable provider challenge dispatch and explicit CREAO gateway registration |

Merge resolutions preserve acquisition model/Apify configuration before Elastic
attachment, both Cargo dependency registrations, and every capability, adoption,
acquisition, finalization, specialist, outbox and projection worker. Source behavior
is preserved; these integration merges do not incorporate the separate failing
review probes or claim their defects are fixed.
The later snapshot `dde0682` adds #26–#28. Its only manual conflict resolution
unions gateway routes with the existing acquisition/adoption/invocation routes.
A separate merge review confirmed that all routes, startup recovery and workers
remain registered, and incoming source behavior was preserved.
Snapshot `1004934` adds #29. Its three manual conflicts were documentation only;
it retains the newer operator-attachment flow and the receipt proof limits.
Provider/runtime/config sources match #29; the only server module change is
registration of `creao_readiness`. No route or worker changed in this merge.

## Validation

The receipt/UI integration at `8014683` passes **112 focused Rust tests across 17 suites**:
`genesis-creao`, `genesis-providers`, and the five server suites for CREAO config,
provider readiness probes, gateway readiness, specialist readiness and jobs.
Formatting and strict workspace all-target Clippy pass. The assembled console
passes **37 tests across six suites** and its TypeScript/Vite production build.
These checks cover the newly combined source; the full Rust suite was not rerun
for this narrow integration. Main's separate #32 head passes 598 tests/65 suites.

The subsequent sibling-agent integration `3961ecc` passes **19 focused tests
across four suites**: the unchanged original duplicate-attachment regression,
existing attachment behavior, saga concurrency and specialist readiness.
Formatting and strict workspace all-target Clippy also pass at this source.
The provider/UI sources and their scoped checks above are unchanged by this merge.

The assembled source snapshot `1004934` passes **699 Rust tests across 87 suites**
with `serde_json/preserve_order`, zero failed or ignored, strict default-feature
all-target Clippy and formatting. This includes #29's concurrent dispatch,
cancellation, restart, callback, purpose-isolation and failed-checkpoint cases.

Before the latest UI fix, `dde0682` passed 33 console tests, a production build
and schema/type regeneration without drift. Schema/type source is unchanged by
#31/#32; regeneration was not repeated. The current 37-test UI check above
supersedes the older console test count.

The full run includes the strengthened scenario fixture (`9d63cc0`). A recording
proxy proves B's actual automatic adoption sends
its committed requirement query through the real Rust Elastic client and receives
the repaired capability identity/source digest. The full native artifact pin is
checked against the canonical registry. Both organizations project/search their
own evidence, cross-mission searches return no foreign hits, and B retains that
configuration after restart. Fresh invocation proofs and both role handoffs remain
required. Gateway receipt and operator-attachment tests also run in the assembled
workspace. The passing suite does not contain the separate negative review probes.

At the earlier `dde0682` snapshot, a fresh development stack also completed `scripts/dev-scenario.mjs` followed by
all eleven checks in `scripts/dev-failures.mjs`. The process used separate temporary
databases and loopback ports, with Elastic environment configuration removed, and
was stopped afterward. These drill results exercise the real kernel with explicit
development stubs; they do not cover native asynchronous provider execution or
browser interaction with the Attention panel.

These are local fixtures: canned model/Apify responses, loopback Elastic/CREAO
APIs and explicit development role bindings. CREAO now has both the kernel gateway
verifier and durable provider probe, registered with complete runtime settings and
`GENESIS_PUBLIC_URL`. No live CREAO account or installed gateway skill has been
exercised. A valid callback proves challenge reachability, not skill installation,
effective agent configuration or mission competence. Fetch's signed local
recruitment proof remains separate from kernel role activation. No live sponsor
execution or cross-account Actor access is established by this branch.

## Open source reviews

Keep fixes on the owned source branches, then merge their new commits into this
integration branch and rerun the affected checks. Avoid force-pushing the shared
checkpoint. The held source PRs remain unapproved and unmerged; capability PRs through #24
landed separately under the user's authorization.

| Source | Required follow-up | Evidence |
| --- | --- | --- |
| #11 | Mission/assignment expiration checks and managed-result finalization guard | [Review](https://github.com/HenryWashuHe/genesis/pull/11#pullrequestreview-5615583092); test-only `b2037e5`, five desired-safe failures |
| #12 | Durable requirement-create identity/uncertainty and status-aware rejected-change recovery | [Review](https://github.com/HenryWashuHe/genesis/pull/12#pullrequestreview-5614141053); test-only `70c4edb`, three desired-safe failures |
| #20 | Managed specialist result guard and authority check before each retry | [Review](https://github.com/HenryWashuHe/genesis/pull/20#pullrequestreview-5614181896); test-only `faf3d8b`, three desired-safe failures plus passing control |
| #21 | Bind displayed acceptance jobs to their installation and refresh active/failed run observations | [Source review](https://github.com/HenryWashuHe/genesis/pull/21#pullrequestreview-5615620684); no new browser reproduction |
| #22 / shared #11 client | Revalidate cached planner configuration and disable Agent Builder redirects | [Source review](https://github.com/HenryWashuHe/genesis/pull/22#pullrequestreview-5615620695); no new redirect/configuration-drift reproduction |
| #26 | Reconcile early receipt run correlation against the trusted dispatch result | [Review](https://github.com/HenryWashuHe/genesis/pull/26#pullrequestreview-5616058167); test-only `8d82c32`, one failure and one passing control |

#27's sibling-agent finding is resolved by #33 at `5952987`; its original
`e161aad` regression is now in the passing suite. Twelve other reproduced Rust
review regressions remain unresolved outside this baseline. #28's UI finding
is resolved by #31 at `17b6b02`, using component rendering, not browser clicks.
Both parents remain held from main by #26/#20.

Claude/Eric retains planner, general lifecycle/saga/outbox, asynchronous role
activation and Elastic ownership. Henry's Claude has claimed the scoped #27/#28
fixes. Codex retains capability/provider implementation and narrow integration.
No whole-kernel credit fallback is active.

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
