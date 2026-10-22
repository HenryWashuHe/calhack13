# Combined integration checkpoint

Branch: `codex/combined-integration`, [draft PR #25](https://github.com/HenryWashuHe/genesis/pull/25).
This is an assembled development checkpoint
for both machines. It is draft because the source reviews below still have open
findings. Passing the existing suite does not resolve those findings.

The [PR review queue](PR_REVIEW_QUEUE.md) distinguishes merged additions from
held source changes. Current combined checkpoint
`f5e747c9f95c27896bb4060b750db7ae502bf946` includes main `f51d4d9`: reviewed
specialist execution #20 with #36/#41, and venue portal #35 with #40. This was a
clean merge with no additional runtime edits. **71 affected Rust tests across nine
server suites**, formatting and strict workspace all-target Clippy pass.
Web/schema sources are unchanged from `cfdaa77`; their checks were not repeated.
This is focused integration evidence, not a new full-workspace total.

#20/#35 are now on main and their reported findings are resolved. #20 no longer
holds the readiness stack; #26/#37's durable reconciliation issue still holds
#27/#28/#29. #27/#28's own reported findings are resolved. Planner fix #38 is now
reviewed and merged into #12, whose main integration is pending; this combined
snapshot does not yet include it. Elastic #39 remains held for its new reproduction,
while reviewed #42 merged into #22 with its dependencies and integration still
pending. Neither fix is included in this combined snapshot. Source ownership remains scoped; no live provider or public portal tunnel is claimed.

## Included snapshots

| Source | Exact head | Purpose |
| --- | --- | --- |
| Main capability stack through #24 | `9e7939a` | Synthesis, bounded repair, durable acquisition/adoption, fresh verification, automatic two-organization scenario |
| Main receipt addition #32 | `050b099` (merge `f03ce9b`) | Bound, read-only CREAO native execution receipts; no domain-success claim |
| #11 `feat/elastic` | `aa94d100` | Evidence projection, retrieval, Agent Builder specialists, Elastic capability search and jobs |
| #12 `feat/planner` | `22fefe62` | Mission planner and persisted decision/reconciliation API |
| Main specialist execution #20 | `456170e` (merge `38aa622`, reviewed tree `5798cb4`) | Managed specialist results, mission/lease and retry authority; backend Actor tools remain usable |
| Main venue portal #35 | `e46dcdd` (merge `f51d4d9`) | Judge-controlled public pages and separate trusted-suite/manifest generation, including #40 |
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
review probes. The latest main merge incorporates the reviewed #20 corrections;
other outstanding source failures remain described below.
The later snapshot `dde0682` adds #26–#28. Its only manual conflict resolution
unions gateway routes with the existing acquisition/adoption/invocation routes.
A separate merge review confirmed that all routes, startup recovery and workers
remain registered, and incoming source behavior was preserved.
Snapshot `1004934` adds #29. Its three manual conflicts were documentation only;
it retains the newer operator-attachment flow and the receipt proof limits.
Provider/runtime/config sources match #29; the only server module change is
registration of `creao_readiness`. No route or worker changed in this merge.

## Current focused validation

Checkpoint `f5e747c` passes 71 affected Rust tests across nine server suites:
20 invocation, eight adoption, eight specialist authority review, four specialist
jobs, nine specialist readiness, four authority, nine gateway readiness, six
attachment and three saga concurrency tests. Formatting and strict workspace
all-target Clippy pass. Web/schema sources match `cfdaa77` exactly, so no repeat
UI/schema gate was needed. The latest merge introduces no additional runtime edits.

Main's separate #20 evidence is scoped to its source snapshots: 620 Rust tests
across 69 suites passed at integration `9bac3e1`, before the final guard move;
53 affected tests across six suites passed at final `5798cb4`, plus formatting,
strict workspace Clippy, 17 web tests/build and schema/type regeneration without
drift. The portal separately passes 16 tests with zero skipped and both suites pass Rust verification against independently
fetched A/B pages. No public tunnel or live provider was used.

## Historical validation

The receipt/UI integration at `8014683` passed **112 focused Rust tests across 17 suites**:
`genesis-creao`, `genesis-providers`, and the five server suites for CREAO config,
provider readiness probes, gateway readiness, specialist readiness and jobs.
Formatting and strict workspace all-target Clippy pass. The assembled console
passes **37 tests across six suites** and its TypeScript/Vite production build.
These checks cover the newly combined source; the full Rust suite was not rerun
for this narrow integration. Main's separate #32 head passes 598 tests/65 suites.

The subsequent sibling-agent integration `3961ecc` passed **19 focused tests
across four suites**: the unchanged original duplicate-attachment regression,
existing attachment behavior, saga concurrency and specialist readiness.
Formatting and strict workspace all-target Clippy also pass at this source.
The provider/UI sources and their scoped checks above are unchanged by this merge.

The earlier assembled source snapshot `1004934` passed **699 Rust tests across 87 suites**
with `serde_json/preserve_order`, zero failed or ignored, strict default-feature
all-target Clippy and formatting. This includes #29's concurrent dispatch,
cancellation, restart, callback, purpose-isolation and failed-checkpoint cases.

Before the latest UI fix, `dde0682` passed 33 console tests, a production build
and schema/type regeneration without drift. Schema/type source is unchanged by
#31/#32; regeneration was not repeated. The 37-test UI check at `8014683` superseded the older console test count.
Neither is a new web test run at `f5e747c`.

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

Keep fixes on the owned source branches, then reconcile their reviewed commits
into this branch and rerun affected checks. #20 and #35 landed separately on main;
this assembled draft still contains other held source changes and is not ready
to merge wholesale. The current [review queue](PR_REVIEW_QUEUE.md) records exact
candidate heads and reproduction commits.

| Source | Remaining follow-up | Evidence |
| --- | --- | --- |
| #11 / candidate #39 | Move Elastic provider override rejection out of shared `new_job` and retain it at generic `/jobs`, as main now does for CREAO | [Review](https://github.com/HenryWashuHe/genesis/pull/39#issuecomment-6346762701); new HTTP composition failure `b3d4082`; 44 existing checks and strict Clippy pass |
| #12 / reviewed #38 | Reconcile the reviewed planner parent with main; combined `f5e747c` still has the older planner source | [Updated review](https://github.com/HenryWashuHe/genesis/pull/38#issuecomment-6346903563); source `ae3879b` and parent merge `d1a5e1d` pass 28 focused tests, including unchanged regression `72e03cd`, plus strict focused Clippy and formatting |
| #21 | Bind displayed acceptance jobs to installation/adoption evidence and refresh active/failed run observations | [Source review](https://github.com/HenryWashuHe/genesis/pull/21#pullrequestreview-5615620684) |
| #22 / reviewed #42 | Own configuration/transport findings resolved; #11/#12 dependencies and integration remain pending | [Fix #42](https://github.com/HenryWashuHe/genesis/pull/42); source `c2b0f11` merged into #22 at `c6ccee4`, with 16 focused tests and strict Clippy passing |
| #26 / candidate #37 | Keep mismatched early receipts provisional through failed reconciliation writes and restart | [Review](https://github.com/HenryWashuHe/genesis/pull/37#issuecomment-6346531598); regression `80c0002` |

Reviewed #38 has landed in its #12 parent; its main integration at `0cad0c4` is
still being tested. Reviewed #42 has landed in #22, with dependency integration
pending. Neither is included in this combined snapshot. #37/#39 have not landed
in their parents or this combined snapshot.
#27's sibling-agent finding is resolved by #33 at `5952987`; its unchanged
`e161aad` regression is included. #28's UI finding is resolved by #31 at `17b6b02`
with component rendering, not browser clicks. Both remain pending the readiness
ancestry, as does #29. No aggregate claim that all negative probes now pass is made.

Claude/Eric and Henry's Claude retain their scoped planner, general lifecycle,
Elastic, readiness and UI work. Codex retains capability/provider implementation,
review and narrow integration. No whole-kernel ownership transfer is active.

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
