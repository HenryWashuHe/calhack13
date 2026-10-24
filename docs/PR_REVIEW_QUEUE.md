# PR review queue — October 22, 2026

Main is `f51d4d9785c8087e37f631021f434a547b343ddc`. The user authorized merging
reviewed ready PRs; the capability stack, execution receipts #32, specialist work
#20 and venue portal #35 have landed. Other findings and dependency holds remain.
Reviewed merges preserve ancestry and use explicit tree checks; no administrator
bypass or squash/rebase was used. This documentation does not merge held source code.

## Latest landings and validation scope

- [#20](https://github.com/HenryWashuHe/genesis/pull/20) merged to main at
  `38aa6228e550d9ddb8d8e86a7a9f45d06576adb2`. Fix #36 first merged into #20 at
  `7693701`; [#41](https://github.com/HenryWashuHe/genesis/pull/41), reviewed source
  `5798cb4`, then merged into #20 at `456170e`. Both the final #20 head and its
  main merge have the exact reviewed `5798cb4` tree. Managed results and retry
  authority are guarded; mission/lease checks apply before preparation, advances,
  retries and collection. The generic provider-override guard stays at `/jobs`,
  allowing CREAO-bound roles to use backend Apify acceptance/invocation routes.
- **Rust evidence is scoped:** integration `9bac3e1` passed 620 tests across 69
  suites with `serde_json/preserve_order` before the final guard move. Final
  source `5798cb4` passed 53 affected tests across six suites, formatting and
  strict workspace all-target Clippy. Its 17 web tests and production build pass;
  schema/type regeneration has no drift. No new full-workspace total is claimed
  for the final source or subsequent portal merge.
- [#35](https://github.com/HenryWashuHe/genesis/pull/35) merged to main at
  `f51d4d9`, resolving its reported findings. [#40](https://github.com/HenryWashuHe/genesis/pull/40)
  source `1316f49` merged into #35 at `e46dcdd` with an identical tree, separating
  generated suite and manifest paths. The main merge matches the expected combined
  tree `74e0185151f675958d3328711ea064d0da048e38` exactly. **16 portal tests pass,
  zero skipped**; two separately fetched A/B pages passed Rust verification. No
  public tunnel or live provider execution was performed.
- Updated [#38](https://github.com/HenryWashuHe/genesis/pull/38), source
  `ae3879b3ebb2dc9dc5bdbd1f72fc1cad6de6e0ae`, merged into #12 at
  `d1a5e1dd4ca83885e1c761074f563328cffaed94` with the identical reviewed tree.
  The unchanged `72e03cd` recovery-gap regression now passes. All 28 focused tests
  pass (six unit, eight planner integration, two HTTP, eight recovery and four
  requirement-idempotency tests), plus formatting and strict focused Clippy.
  [Independent review](https://github.com/HenryWashuHe/genesis/pull/38#issuecomment-6346903563)
  found no remaining recorded source blocker. #12's integration with main is being
  tested at `0cad0c4` on `codex/planner-main-integration`; it has not merged to main.
- [#42](https://github.com/HenryWashuHe/genesis/pull/42), source
  `c2b0f1107d4ff6049e7b78fa6a2ffb52fbed12da`, merged into #22 at
  `c6ccee4c77229d0f187c568f3564c2659e22d935` with the identical reviewed tree.
  All 16 focused tests pass (four configuration drift, one planner, nine Agent
  Builder and two redirect tests), plus strict focused Clippy. #22's own findings
  are resolved; #11/#12 dependencies and integration remain pending.

## Earlier reviewed additions

- [#32](https://github.com/HenryWashuHe/genesis/pull/32), source `050b099`, merged
  to main at `f03ce9b`: read-only CREAO execution receipts bind the frozen
  job/attempt/assignment/agent to the backend journal. That identical source/merge
  tree passed 598 Rust tests across 65 suites, formatting and strict all-target
  Clippy. Venue-continuity domain verification and live mission execution remain
  separate work.
- [#31](https://github.com/HenryWashuHe/genesis/pull/31), source `a83357a`, merged
  into #28 at `17b6b02`: awaiting readiness keeps Abandon and hides Attach.
  28 web tests, typecheck and build pass; component rendering is covered, not
  interactive browser writes.
- [#33](https://github.com/HenryWashuHe/genesis/pull/33), source `140c26f`, merged
  into #27 at `5952987`: sibling roles cannot attach the same native agent.
  The unchanged original `e161aad` regression and 18 related tests pass, along
  with formatting and strict focused Clippy. #27/#28's own findings are resolved;
  their readiness dependency remains pending, while #20 is now on main.

Henry's Claude [claimed #27/#28](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6345636513)
and supplied #31/#33. The scoped specialist follow-up #41 and output-path fix #40
were Codex work; neither changes the broader source ownership split.

## Historical capability-stack landing

The following dependency order was used. All reviewed source heads and their
ancestry were preserved.

| Order | PR | Reviewed head | Merge commit | Main review focus |
| --- | --- | --- | --- | --- |
| 1 | [#8](https://github.com/HenryWashuHe/genesis/pull/8) | `2a006e2a85dd237ac9f37320704ee7e19707a478` | `5c8c4b6` | Persisted model intent before POST, prompt/receipt binding, bounded transport, no ambiguous replay |
| 2 | [#9](https://github.com/HenryWashuHe/genesis/pull/9) | `3af3551718c9f678fb667a9ebdae625449141b40` | `f9f20b3` | Repair lineage, semantic-only repair gate, atomic transitions and completion capacity |
| 3 | [#10](https://github.com/HenryWashuHe/genesis/pull/10) | `343789f22d1db3286bd945cd0d8e1da33a2d0c06` | `b1abe6a` | Authority/version checks, atomic quota debit and saved effect intent, post-call fencing |
| 4 | [#13](https://github.com/HenryWashuHe/genesis/pull/13) | `248e8af33d3c53241a37417985530a9f9818dec4` | `6d35ecb` | Mission/lease deadlines, saved assignment identity, acceptance surviving retirement, bounded manual scenario |
| 5 | [#14](https://github.com/HenryWashuHe/genesis/pull/14) | `b364226e27d1b74cb9cc79d90238fa805935e502` | `f318cdb` | Fresh invocation evidence, historical proof restoration, atomic proof/job/event commit |
| 6 | [#15](https://github.com/HenryWashuHe/genesis/pull/15) | `11c76d122518ea5d3cd8b44d3709da556c77a233` | `ea96b06` | Transactional finalizer claims, bounded restart recovery, manual/worker convergence |
| 7 | [#16](https://github.com/HenryWashuHe/genesis/pull/16) | `4c428ca01ba6a3e3d403797cdf5444505fa30824` | `a36aa19` | Scheduled-version fencing, local paused-work reconciliation, unknown POST and failed-read gates |
| 8 | [#17](https://github.com/HenryWashuHe/genesis/pull/17) | `6d089be417c70f12b967ef9347ecd7c3a6d2ade2` | `ab1f5f9` | Automatic observation without manual fallback, version-skew handling, immutable proof requirement |
| 9 | [#18](https://github.com/HenryWashuHe/genesis/pull/18) | `4e37944db99d87aca7d3e040b1359ac0523d07d9` | `d19b1ca` | Atomic history, resumable baseline migration, stable pagination, redaction |
| 10 | [#19](https://github.com/HenryWashuHe/genesis/pull/19) | `7df90de6cd8b81e5b095ba5ffec050be5e4a9593` | `94d2e51` | Requirement-bound selection, atomic installation/check batch/acceptance, child authorization, terminal recovery |
| 11 | [#24](https://github.com/HenryWashuHe/genesis/pull/24) | `9e7939a0f060784db282a8269ce2e5acbbe6554a` | `4ecc397` | Independent B-only adoption intent, frozen selection checks, adoption-bound evidence and fresh invocation proofs |

The historical stacked head has prior local evidence of 594 passing Rust tests across 64
suites with `serde_json/preserve_order`, formatting and strict default-feature
all-target Clippy. The user's subsequent merge authorization applied to these
reviewed heads. Validation does not apply automatically to a changed head. GitHub's CodeRabbit summary status is not
a build/test CI result or a comprehensive code review.

Known boundaries remain: one process owns each organization database; retained
checkpoints require compatible trusted configuration; invocation verification
uses trusted inputs; failed reads and ambiguous POSTs require explicit recovery.
The scenario is an operator-driven demonstration of structural evolution, not
proof that the planner independently discovered and solved the entire mission.
Cloud execution and cross-account provider access remain separate evidence gates.

## Open source fixes and dependency holds

Keep fixes on their source branches. Passing original regressions does not approve
a candidate that introduces a new failure. The current candidate blockers are:

| Parent / candidate head | Remaining blocker | Review evidence |
| --- | --- | --- |
| [#11](https://github.com/HenryWashuHe/genesis/pull/11) / [#39](https://github.com/HenryWashuHe/genesis/pull/39) `49ca9aca2c9ef79bf43c00c63cb872d2ab138f6c` | Provider override guard in shared `new_job` blocks backend Apify acceptance/invocation for Elastic-bound roles; compose with main's #41 HTTP-boundary fix | [Review](https://github.com/HenryWashuHe/genesis/pull/39#issuecomment-6346762701); test-only `b3d4082934f6220e058d6b3697ae14f12c69d3d4` reproduces HTTP 409 at backend checks. 26 Elastic + 18 server checks and strict Clippy pass, including the original authority regressions. |
| [#26](https://github.com/HenryWashuHe/genesis/pull/26) / [#37](https://github.com/HenryWashuHe/genesis/pull/37) `5e6072d25b3ccc5ce7c4b5cbee88c1d318f39c46` | Failed mismatch-retirement write leaves a provisional receipt eligible for the next pass's Ready shortcut | [Review](https://github.com/HenryWashuHe/genesis/pull/37#issuecomment-6346531598); test-only `80c000232c8d13fb8ebcdb2331b448656ca315a1`. 23 ordinary readiness checks and strict focused Clippy pass. |
| [#21](https://github.com/HenryWashuHe/genesis/pull/21) `2c987193892da94e4110179362f2ef2aa052779c` | Bind acceptance runs to installation ID, support adoption acceptance purpose, and refresh changed/failed evidence reads | [Source review](https://github.com/HenryWashuHe/genesis/pull/21#pullrequestreview-5615620684); no new browser reproduction. |

The held parent heads remain #11 `aa94d10` and #26 `3dc95d6`; the candidate fixes
in the table have not landed. Updated #39 head `1bf7304` is under review; its
previously reviewed `49ca9ac` finding remains recorded above. #12 now includes reviewed #38 at `d1a5e1d`, with
main integration pending. The earlier `ed0f074` gap failure is historical and is
resolved at `ae3879b`. #20 is merged and no longer holds #26.
After a durable readiness correction is reviewed, reconcile the ancestry of
#26 → #27 → #28 and sibling #29 with current main; resolved child findings do not
remove that dependency or approve an unreviewed merge result.

- [#22](https://github.com/HenryWashuHe/genesis/pull/22) `c6ccee4`: own configuration/transport
  findings resolved by #42; pending #11/#12 dependencies and integration.
- [#27](https://github.com/HenryWashuHe/genesis/pull/27) `5952987`: own sibling-binding
  finding resolved by [#33's review](https://github.com/HenryWashuHe/genesis/pull/33#issuecomment-6345993608); pending #26.
- [#28](https://github.com/HenryWashuHe/genesis/pull/28) `17b6b02`: own UI finding
  resolved by [#31's review](https://github.com/HenryWashuHe/genesis/pull/31#issuecomment-6345825851); pending the readiness ancestry.
- [Draft #29](https://github.com/HenryWashuHe/genesis/pull/29) `c1e6ba4`, based on
  #26 at `3dc95d6`: the provider readiness probe passed its scoped review and
  407 Rust tests, formatting and strict Clippy. It remains draft for #26/#37's
  durable readiness issue. Native completion does not establish readiness or
  prove skill installation; no live provider run was made.

## Documentation and historical combined checkpoint

- [#23](https://github.com/HenryWashuHe/genesis/pull/23) `665aab4` still needs its
  stale handoff claims refreshed against current main and this queue. Specialist
  registration is now on main through #20; gateway readiness #26 and the durable
  provider probe #29 remain pending. Live skill/account proof remains absent.
- [Draft #25](https://github.com/HenryWashuHe/genesis/pull/25) now includes main
  `f51d4d9` at combined checkpoint `f5e747c`, a clean merge with no additional
  runtime changes. 71 affected Rust tests across nine server suites, formatting
  and strict workspace all-target Clippy pass. Web/schema sources are unchanged
  from `cfdaa77`, so their checks were not repeated. [Current combined guide](COMBINED_INTEGRATION.md).
  The earlier `1004934` snapshot passed 699
  Rust tests across 87 suites, strict Clippy and formatting; that is not a
  current-main or updated-combined full-workspace validation claim.
  The earlier `dde0682` web/schema evidence was 33 web tests, production build and
  schema/type regeneration without drift. Original negative review probes were
  outside that passing baseline; the remaining candidate failures and pending
  integration checks above are separate gates. [Historical validation scope](https://github.com/HenryWashuHe/genesis/blob/8a7b115/docs/COMBINED_INTEGRATION.md).

Before landing held work, verify the exact proposed head, rerun its affected
regressions and inspect any integration changes. Preserve ancestry when reconciling
with main; do not merge the combined draft wholesale while its findings remain.
Passing fixtures do not close live-provider gates. Codex owns capability/provider
implementation, review and narrow integration; Claude/Eric and Henry's Claude
retain their scoped planner, general lifecycle, Elastic, readiness and UI work.
No whole-kernel ownership transfer is active.
