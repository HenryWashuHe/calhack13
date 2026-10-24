# Implementation status

## Active repository checkpoint

Work and reviews belong in [genesis-history-experiment](https://github.com/HenryWashuHe/genesis-history-experiment)
and [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
Use the [migration map](REPOSITORY_MIGRATION.md) for legacy PRs. The
[ongoing goal](ONGOING_GOAL.md) remains a bounded A → B → A executable
development-method loop and honest controlled evaluation with fixed model
weights. Event organization remains superseded. G0–G4 are still open.
The revised app Goal was activated on October 24, 2026 after the stale record
was cleared; the existing hourly continuation remains active.

Main through `d18591ad6eb97f0eb59a3c70dea097878f8fb10b`, tree
`e457ac0b472fe7531906e042ae2f1d9638e12810`, includes the reviewed
foundations below. Method generation merged at `d1be8443fee46e1196861926a963563f48407936`,
whose tree `dc9d6adc19863d8584fd5af1fd11eae7552014eb` exactly matches the independently
validated generation stack. Counts refer to the named source/composition, not a
combined main run.

| Work | Validation and evidence boundary |
| --- | --- |
| Revision input #150 `6376713a` → `a5f87bfe` | Root 18 focused tests and scoped strict Clippy/fmt pass; independent source/guide review clear. [Generic bounded bundle](invention-lab/contracts/revision.md#bundled-input) delegates existing prepare/observe logic, preserves legacy identities and refuses conflicting modes before effects. No provider/source execution. |
| Selected-source bridge #151 `54283b57` → `d18591ad` | Behavior `7d0d30f`: root 25 scoped tests (five new including two independent, four legacy CLI, 16 binary) and strict Clippy/fmt; independent final source/guide review clear. [Exact fetch → retained revision input](invention-lab/contracts/method-registry.md#selected-parent-to-revision-input) preserves receiver fields and publisher-qualified metadata. Original 88-line guard retained, including 14+2 inputs and overflow refusal. Prepared only, no model run, adoption or G2/G3 closure. |
| Pure core verdict #146 `0c9a5533` → `c6870504` | Existing 253-line decision implementation moved to core, equivalent modulo import prefixes; scorer compatibility re-export and three original regression files unchanged. Root full core tests, strict all-target core Clippy/fmt and standalone scorer `cargo check --tests` pass. Scorer check was compile-only; no lab tests executed in review. Prerequisite for Henry's authority slice, not acceptance/authority evidence. |
| Method exchange CLI #141 final `e938db0` → `ec28434b` | Behavior `be639af`: four CLI plus 16 binary tests pass on the final composition; four CLI cases independently rerun, scoped strict Clippy/fmt, overlapping counts. [Thin existing-client commands](invention-lab/contracts/method-registry.md#operator-cli) publish/search/fetch an explicit selection; JSON stdout stays separate from debug logs. Loopback inert source only, no method execution, adoption or G2 closure. |
| Registry boundary guards #142 `59faaaf` → `f2b33d50` | Three independent cases and scoped strict Clippy/fmt pass; original 144-line file unchanged. Exact-limit truncation, applicability count and lookup validation boundaries. Test-only; mutation results remain author-reported, and literal-query completeness is not semantic absence. |
| Public seed ranges #121 `7981a46` → `58b8ba50`; receiving wrapper #136 `5d71188` → `03c16cb3` | Seven scoped metadata/CLI preflight tests pass; original 67-line range guard unchanged, new 47-line CLI guard. Wrapper composition passes nine committed mocks; two additional independent mock checks pass, original 203-line regression unchanged. The CLI rejects invalid ranges before candidate reads/output creation; the wrapper reads package inputs first and rejects before output creation/subprocess calls. No method/scorer/namespace execution or evaluation result. |
| Typed method client #133 `1f94f312` → `995a7d70` | Author 19 focused cases at `125e260`; six final client cases independently rerun, counts overlap. Strict all-target server Clippy/fmt on unchanged production. Full selected-candidate/source identity, bounded strict responses, no redirects/retries and duplicate-dependency rejection before a request. [Loopback transfer](invention-lab/contracts/method-registry.md#typed-organization-client) reaches B revision `Prepared`, not generation, adoption or authority. |
| Chain exact-fit guards #134 `edf1a56` → `7955c957` | Original 76-line file unchanged; two independent cases, scoped strict Clippy/fmt pass. Exact call/output capacity is accepted and one-unit-short capacity refused. Test-only; the author's mutation campaign remains author-reported. |
| Method registry API #132 final `bc1cdd9b` → `26b6cc87` | Production `2039c5a4`: 14 author checks/3 targets plus three independent transport cases pass; scoped strict Clippy/fmt. Original 224-line `703fcfd` file unchanged. Reuses registry process/SQLite/auth for immutable publisher-qualified source, bounded keyword metadata search and full-reference fetch. Local authenticated HTTP fixtures only; no source execution, adoption or authority claim. Typed client #133 is recorded separately. |
| Read-only chain-cost CLI #130 `04e22315` → `fc16ba14` | Independent 21 focused tests/6 targets, scoped strict Clippy/fmt/diff pass, including a permanent 106-line retained-#113 regression. Author 247 tests/48 targets cover unchanged production source; no full 248-test claim. [Original-file assessment](invention-lab/contracts/chain-allocation.md#read-only-chain-cost-cli) retains missing stages as unknown and rejects invalid evidence without writes or provider/runtime calls. Report construction is not acceptance, enforcement or gate closure. |
| Whole-chain cost assembly #127 `f5fad35` → `f335cc6` | Full forge 243 tests/47 targets, zero failed/ignored; strict all-target Clippy/fmt. Includes six author and three independent cases, preserving the independent 207-line file. [Pure original-record aggregation](invention-lab/contracts/chain-allocation.md#whole-chain-observed-costs) retains unknowns/local overruns and rejects duplicate identities. No provider/runtime work, chronology, exhaustive billing or gate closure. |
| Catalogue audit #124 final `ed573c69` → `2f9cfea9` | Author source `e6940b22`: 142 core tests pass. Independent 11 focused tests pass, including three unchanged `1c2bfd29` cases; scoped strict Clippy/fmt pass. [Post-run full-reference membership](invention-lab/contracts/catalogue.md) retains missing/unknown/failed/overrun evidence. No byte verification, chronology, runtime enforcement or gate closure. |
| Real-evidence UI #123/#128 | #123 `7196d5c` merged into its parent branch at `4515f728`; #128 final `2f0b6734` integrated identical web bytes into main at `a3be74d0`. Independent 81 UI tests/13 files and typecheck pass on the final composition; original regressions unchanged. Folder deduplication, package bytes and inspection identity holds cleared. Display correctness is separate from organization authority or adoption. |
| Explicit reasoning policy #120 `a2c4ff15` → `70a00d51` | Independent `f3a74b58`: 42 tests/10 targets; author 37 tests/7 targets overlaps. Final eight focused tests and strict all-target Clippy/fmt pass. Three `80f8544` cases are unchanged; eight historical checkpoints and four roster/admission pairs retain identity. Sonnet 5.5 adaptive effort pins client choices only; no live request, fixed hosted weights or G4 result. |
| Evidence manifest UI #109 `2cfb4058` → `5ed65deb` | 66 focused tests/11 files and typecheck pass, including all five original `f2ac45e5` cases/70 lines unchanged. Format and full inspection-attribution holds cleared; later real-evidence intake #123 is recorded above. |
| B receiving #116 `59cbf0ad` → `e091ce0d` and A return #117 `3c1994f` → `8eed5043` | Independently checked retained records: B child 3/6 loses to random 4/6 and parent 6/6, beats fixed cart 2/6. A-return comparisons 0–3, 0–1 and 1–3 all Rejected. Separate cost scopes and read-only review; no accepted return, local adoption, Linux assurance or G0–G4 closure. |
| Generated B source revision #112/#113 (`163e952` / `e5d7ece`), result merge `4bcd5c5` | Preparation merged before one request. Independently checked 34 final hashes, 23 unchanged prepared entries plus their manifest, 33 JSON records and six byte-identical read-only CLI outputs; all 36 input files unchanged by review. Child `4f455dc6`, changed source bytes and AST parsing only. Operator-assigned exploratory rough-transport work; no execution/scoring in that source bundle, adoption or G3 closure. |
| UI evidence labels/organization #93/#94, retained guards #115 | #94 `7ed70f9` → `0398dc56`: composition `ef484cbf` passes 21 focused checks/typecheck, including all six unchanged `78adef37` cases. #115 `e9c25f38` → `54cb42e` retains label cases; main preserves the full 121-line original file. Both holds cleared. |
| UI ledger/scoring #99/#100 (`f09bf473` / `76c2cd54`) → `302397ae` / `d11f01fc` | 42 focused checks/typecheck pass, including four unchanged #99/#100 cases, four actual Rust receipt hashes and 11 SHA vectors. Exact arithmetic and receipt attribution corrected; #109 clearance is recorded above. |
| UI lineage/checkpoint #101 `ed8b64de` → `95256998` | 52 focused UI checks/typecheck pass; reviewed `db10b04` and merged tree `ec822b18` match. Complete 119-line independent `59c3e1c` file is unchanged. #109 clearance is recorded above. |
| Public feedback #102 `c8bf7a0` → `7accefd3` | Eight focused tests independently pass with strict Clippy/fmt, preserving `8ad9d645`. Hidden-policy and byte-cap holds cleared; read-only regeneration retains all 24 live-record experiments in 18,152 bytes. No source execution, adoption or revised-method result. |
| Optional chain generation guard #110 `4b57cfa` → `0f277d7` | Independent 34 focused tests pass; author 33 focused tests and strict Clippy/fmt. Optional retained-chain validation precedes recovery/credentials/reservation/provider calls. Counts overlap; cooperating commands only, no runtime-entry or global enforcement claim. |
| Retained chain CLI (`f8b6919`, composition `42d15cd`) | Create-only whole-chain declaration and per-stage bindings; read-only checks reuse existing generation allocation and trial bindings. Full forge 221/42 plus independent 20 focused checks, strict Clippy/fmt. [Contract](invention-lab/contracts/chain-allocation.md#retained-cli-files). No automatic runtime consumption or controlled comparison. |
| CREAO blank-version compatibility #96 source `dbec006da117a5a9f84389d85f27956f997bdf21`, merge `04b4c3756b03a4f5b3c7c7627a48717ccfdc7697` | Independently 68 CREAO Rust tests and 10 Node gate tests pass; the 10 Node cases were also separately rerun. Strict Clippy/fmt are author-validated. Unset/blank selects v1 consistently; explicit v2 stays opt-in and invalid nonblank versions fail. Offline only; no hosted completion or changed saved identities. |
| Stage prerequisite / active-goal checkpoint #97 source `202e1fce4fb27418d6fe4898b6f57545de92f342`, merge `2dfc0993132385cc83227848b722fef08a7d01f0` | Ten independent stage/CLI/overflow/recovery cases pass; scoped strict Clippy/fmt. Existing checked arithmetic is shared through read-only accessors without changing valid serialization/hash domains. Current docs record the revised active app Goal; no gate closes. |
| Finite chain ceilings #103 source `7b65b066687c8636fce2a41097c29362b8bce8e5`, merge `c458c6eb49a4e3cbdc2cc2710930b0252eb29668` | Reviewed and merged tree `4a4a8b00585a894272910df082d7f52f7e7436da`. Author full forge run: 216 tests across 41 targets before three assertions were strengthened; strengthened five chain cases and final scoped strict Clippy pass. The earlier fmt pass preceded a final module-order cleanup; this checkpoint corrects that formatting-only issue and passes workspace fmt. Independent frozen-head review reran 15 chain/stage/CLI/recovery tests, all passed, zero ignored. Counts overlap. [Structural generation/trial-only ceilings](invention-lab/contracts/chain-allocation.md), not runtime enforcement or retrospective allocation. |
| Execution/scoring records #18/#22 (`07961f1` / `7510b86`) | Composition `785a22c` / `4d64fe6`: 102 core tests and strict Clippy. Experimental records, not an executor. |
| Trial plan #28 (`e47ee64`, source `f52f182`) and experiment ledger #32 (`8578759`, source `0fa3773`) | Composition `d386fb0` / tree `680cf27e`: 116 core tests and strict Clippy. Declared attempts, cumulative reservations/settlements, unknown usage and exact transcripts are represented. |
| Source packages #36 (`2b2ecac`, source `f856a5b`) and probes #40 (`df945c9`, source `78dde86`) | Composition `a735bc7` / tree `0125f83c`: 125 core tests and strict Clippy. Exact received bytes, ordered dependencies and host-selected runtime/protocol; no execution or adoption. |
| Simulator #37/#38/#39 (`dc574135` / `3da2d4f` / `965ffb34`; sources `858ca41` / `7a16825` / `5f1e151`) | Composition `949eb569440da6b4a81b5d1b2ca3fc7cc1774839`, tree `2596db96354f10ad0bc5df857c9303105a200bae`: 22 tests, strict all-target Clippy and fmt pass on ARM64 debug. Preserved geometry/weld regressions are unchanged and pass; full-shape bounds and the torque ceiling are checked. Local simulation primitives, not real engineering or cross-platform proof. |
| Executor foundations/session #42/#43 (`a76735d` / `58bcaef`; sources `638ac262` / `53054d50`) | Composition `a1e2afdce49294c7b962bdbbe96836e50d21982a`, tree `5e0243f2875a929548f8eafa2baf9209f1215380`: 19 portable tests executed; one Linux namespace test self-skipped on the nonroot Mac. Strict Clippy/fmt pass. Unchanged `b73b62d` finalization/sealed-submit probes and `3784da9` reply-backpressure pass. Experimental foundations only. |
| Evaluator/adoption proposals #13/#14 (`728d42c` / `407b692`; #14 source `505312ad7efcee57439efc6bf2f054c8899e0c73`) | Corrected holdout protocol, exact evaluator identity, required candidate/control acceptance and receipt boundaries. Reviewed proposals do not close runtime integration or G0. |
| Adoption outcomes/refusals #11/#12 (`eefde296` / `7010866`) and decision UI #15 (`b94af6d`, source `fd60f1a2a6de33daf30afc87f80544c1833681a9`) | #11/#12: 70 focused tests and strict Clippy. #15: 13 focused tests and typecheck, preserving `ae6ba16` unchanged. Local refusal wording and historical/asynchronous evidence-display regressions are corrected. |
| Method generation #50/#51/#52 (`3da52d2` / `1533e2b` / `d1be844`; sources `e5eadd4` / `bc8eca2` / `d6c331c`) | Exact merged tree above: 125 forge tests, strict all-target Clippy/fmt independently pass. Frozen complete request, durable intent before one POST, retained measured/unknown completion usage and exact package export. Legacy parser contracts stay unchanged; no live model call or method execution. |
| Generation guards #63 (`46fd572`, source `60ac021baff05fd5fb5a8c70d33bd64c204ce297`) | Seven new cases and full 144-test serial forge run independently pass; strict Clippy/fmt. Henry's test files are unchanged. Mutation campaign remains author-reported. Initial parallel setup failures are recorded in #67 below, not attributed to these guards. |
| Parallel CLI fixtures #67 (`14fbdc1777b5df169fbccc6c2a4c5e7ca58e811e`, source `0b861365f715c1312eea0ec70f1244dae025e6ab`) | Test-only counters prevent same-process PID/time directory collisions. Original accounting run stopped at 86 passed/1 failed; accounting-only correction then exposed generation fixture collision at 95/1. Final default-parallel forge run: 144 passed, zero failed/ignored, strict all-target Clippy/fmt. Counts overlap earlier runs. |
| Parent-bound revision #66 (`22e73fa3e90fab0bee3ae4b2d6591fe4f924096a`, source `19f2a24036f2e1cc10845a57ae6aa470ba950ad5`) | Source tree `93172bd780be5901681e1ae2e5cad0f2e5ab3c82`: seven focused revision cases independently pass; author full 134 core tests and strict Clippy/fmt. Exact source inspection without execution authority, frozen parent input and whole-plan matching; source relabeling alone is not a change. Synthetic completion only, not HTTP/model/execution or recursive improvement. [Contract](invention-lab/contracts/revision.md). |
| Revision CLI #68 source `49c14958751a4f0fc335623d507ca74a7474381c`, merge `9c3633d69b4ac5fa618b083252542779c93d0d74` | Author full default-parallel forge: 156 passed, zero failed/ignored across 22 targets; five new CLI cases independently pass with scoped strict Clippy. [`method-revise-prepare` / `method-revise-observe`](invention-lab/contracts/revision.md#cli) expose exclusive offline preparation and read-only source derivation. No model call or source execution; overlapping counts. |
| Adoption and rehearsals #64/#70/#72 (`181b816` / `8561640` / `751b0c6`) | Independent composition `f527e69c21685e1a03764828e4f3fef43a398f82`, tree `1855f3a8db02104736bc19e54e3961b95aa2ac26`, matched main after #72: two rehearsals and seven adoption cases pass, zero ignored. Both rehearsals Rejected at 0/3 versus 0/3. Exact-parent/receiver binding and retained-file cost CLI equality are fixtures, not actual model generation or a recursive-benefit result. |
| Generation roster #73 source `390a8676f1f16c28c2a9ce91463965c85cd53d94`, merge `67b271344a479fba71313a521742ba0af858e2de` | 18 focused cases independently pass with strict Clippy; a separate historical-request continuity probe covers all six generation phases. Source-free pre-result declaration and exact matching retain all declared attempts. No provider budget enforcement, authenticated chronology, completeness or benefit claim. [Contract](invention-lab/contracts/generation-roster.md). |
| Offline roster CLI #74 source `2c80890c9694a344938f2e9e8b26d3653cdb6bf1`, merge `86ab829908cd6d2a4c5bf1da20d54de35574e091` | Reviewed tree `5a44f91a489d7bbb088c24d1fbd0f94b2ade3ce1` matches the merge. Author full default-parallel forge run: 167 passed, zero failed/ignored; strict all-target Clippy. Four CLI cases independently pass. [Prepare/assess commands](invention-lab/contracts/generation-roster.md#offline-cli) are offline; assessment is read-only and exit 0 is report construction, not budget compliance. |
| Independent roster guards #75 source `eb41b81f4b94503ff901c5a7d52474440f65d6f7`, merge `509b971e95047f758bd181c8c369e2630858b9da` | Two independent files preserved unchanged: historical saved-request continuity across all six phases, plus CLI settings mismatch/output-alias/substituted advanced-snapshot boundaries. Both tests rerun and pass; forge formatting passes. Final code tree `a31ad0c9bf5f34b58888f83b3b6caf94f712dc40`. These are separate, overlapping checks; the full 167-test run belongs to #74's exact source. |
| Generation reservation library #77 source `197cb17dbfca5c47d38c1f03e950979e9b6b2564`, merge `fd83c3cf208de2db2565a8932ee598d0e0f651cf` | 12 focused cases independently pass with strict Clippy/fmt. A source-free journal reserves each exact declared Prepared identity once, retaining the full reservation after failure, rejection, uncertainty or unused capacity. Request count and requested maximum output are bounded per trusted journal; this library performs no IO. [Contract](invention-lab/contracts/generation-admission.md). |
| Optional generation-step guard #78 source `0f89d25a8d0589f5dcb9c63f8d0dde264c4a67c3`, merge `80ae988c58580160d2e2c81e89eb662e37a506a8` | Merged tree `91205ae7e6e4b9a8ff3dba6530f1d4984f66370a` matches independently reviewed composition `ea1ee023`. Author full default-parallel forge: 179 passed, zero failed/ignored across 28 targets; strict Clippy/fmt. Independent reviewer reran 27 focused cases successfully. `--admission` persists the reservation before Requesting/provider submission through the existing step; legacy ungated calls remain available. [CLI contract](invention-lab/contracts/generation-admission.md#cli). |
| Independent admission guard #79 source `d2b5c0fce04bad08e383dac8f99dc1bfd80b094d`, merge `666cf77f22b674e5114cff4c60258b22dee7caa1` | One 155-line test preserved unchanged and independently rerun: occupied session lock, missing journal and path aliases fail without modifying inputs or reserving capacity. Formatting passes. Final forge subtree `7aceb366734317ad1ec416fa626e8f12f50665dc` matches the reviewed source. The full 179-test result belongs to #78; no full 180-test main run is claimed. |
| Admitted trial/cost join #81 source `3d66d78606e6d1b26ba5846acb45993fddd7d594`, merge `0cfa80b626229c6340a17c4da539a14084f40d62` | Reviewed/merged tree `186aad7ffe3b7be3c2c75ceac8764433a53565b0`: 25 focused cases independently pass. Full original roster, historical identities and advanced-attempt reservations are checked before the existing cost assessment; generation and combined grants remain separate. [Contract](invention-lab/contracts/admitted-costs.md). No pre-spend trial allocation or full adaptive accounting. |
| Admission-aware cost CLI #82 source `132b99a295a50d8fa692f09219044b97570e5476`, merge `95a0ce6c9dff52cde40bb7306697ebfa20457e17` | Tree `40df89a27e10863c1d65e12a39a65f1ed81f5f0f` matches the reviewed source. Author full default-parallel forge: 188 passed, zero failed/ignored across 31 targets. Independently: 15 focused CLI/compatibility cases and strict all-target Clippy/fmt pass. Optional [`--admission`](invention-lab/contracts/admitted-costs.md#cli) fails invalid evidence without fallback or writes; legacy behavior remains unchanged. No provider calls or source execution. |
| Independent admitted-cost guards #83 source `9a47daf0da39eeecf70ccc12e141c6128ba5c203`, merge `613bdca69eed5b502c7ccd358a85890d12e78574` | Two files remain byte-identical to independent sources `e3e8bdb62e46905610749084f3ac5a4c82af61cb` and `bc0288879477fe9d581200dc1dec9b47f9e92b03`: historical Failed-session identity and CLI malformed/oversized/symlink/path/immutability boundaries. Both cases independently rerun and pass; fmt/diff clean. Final forge subtree `88326cba69b0ec61958e56fdcd01a4ffc49d6daa` matches the tested source. Full 188 belongs to #82; these two separate checks are not a full 190-test main run. |
| Stage allocation #85 source `e468179e2d9ffce0ec0af1a883dc3f2a511dbcc1`, merge `1226ff1b6d326eaed72f0e4cc01b634f2cca56b6` | Reviewed/merged tree `d6778aba80b5a512f1abbb50b760673a8cf11fab`: 21 focused cases independently pass; strict Clippy/fmt. [Structural envelope and later exact binding](invention-lab/contracts/stage-allocation.md) check full-generation plus bounded per-slot grants in all five dimensions. Library performs no IO or runtime admission. |
| Stage CLI #86 final source `ba3a1e2a498441b7ed08480e2e38fed8213d2065`, merge `10b77d48172571c1b1b7aff7a1c48b40973ca2b5` | Reviewed/merged tree `ca9f058995e7b5277222ba3dd56884294c8334b8`. Behavior-identical original `4c34b0b7d4955cfe198bd5e9a6c7f34fa57db8d1`: author full forge 198 passed, zero failed/ignored across 35 targets; independent 19 focused cases and strict all-target Clippy pass. Final formatting-only delta passes fmt/diff. [Create-only allocation/binding and optional guards](invention-lab/contracts/stage-allocation.md#cli-and-retained-files) are cooperative local-file mechanisms, not runtime entry enforcement. |
| Independent stage guards #87 source `249fcdf7e30229af6724f4e65ff858d6d1c01fa0`, merge `a90ad14e7495be1ef5066234f84bddba1ac431b9` | Original `ae493f2c94aa0077ad12fb6fd136e726b4b8cb93` (66 lines) and `327c960edccbf094f1a04d1e9a57206600979d1f` (143 lines) unchanged: final-addition overflow and allocation-before-pending-recovery boundaries. Both separately rerun and pass; fmt clean. Final forge subtree `2c1dac57cb2ef489f42d4baec286e81f1f715082` matches the tested source. Full 198 belongs to the original #86 source; no full 200-test main run is claimed. |
| Core guard coverage #53 (`761eb2b`, source `c614bb13d03c2f9d8550358f86b1ba79683156e8`) | Henry's 173-line `eacfe0f` test file is preserved exactly with authorship (blob `c537e1e9755284ad9b51a79058e19f2ee469a197`). Reviewed composition passes 130 core tests and strict Clippy/fmt; five new cases independently pass separately. Mutation results are author-reported, not independently rerun. Test-only change. |
| Accounting identity #56 (`2b69c25`, source `245cbadef63cc3051ef71bc24554234534275f8e`) | Author-validated 134 core tests and strict Clippy/fmt; four new regressions independently pass. Identity-only checks permit retaining overruns without relaxing existing acceptance/budget validators or changing valid hashes; malformed usage cannot hide behind an early overrun. This is not total-cost integration. |
| Declared combined cost #58 (`04029c56`, source `b27ae8a1f504d53effef42d3c12b1141d3299577`) | Author full forge suite: 128 tests and strict Clippy/fmt. Three new accounting cases and source independently reviewed. Exact generation/checkpoint/trial links, unsuccessful or unused generation and combined known/unknown/overflow/overrun costs; trusted declared roster, not proof of exhaustive history. |
| Accounting boundaries/guide #59 (`2cc05a65`, source `458ceab337c351cca80de005a5c1e9a41429b7b3`) | 15 focused cases independently pass: three accounting, six boundaries and six generation; author strict Clippy/fmt. See [the accounting contract](invention-lab/contracts/accounting.md). Counts overlap the preceding full suite; no provider execution. |
| Offline accounting CLI #60 (`6da4a96`, source `19592bd5039bb18243abc2794d7a8e96ecb6cc39`) | Reviewed tree `1ca02117479f8e09dfc6c9f214372ff37df3df9c`: 18 focused tests (12 existing CLI, three generation CLI, three accounting CLI), strict all-target Clippy/fmt pass. A second reviewer reran the three committed tests and nine temporary input/path/immutability probes without failures; counts overlap. [Read-only usage guide](invention-lab/contracts/accounting-cli.md). |
| Stage #20 (`7023077`, source `bfe10d9`) | Composition `7508c5b`: 15 VM tests, preserving independent files. Neutral startup, explicit fixture replay, ordered live updates and exact manifests; no browser/cloud proof. |
| Apify metadata-lag reads #47 (`48858e6`, source `86a9894bd06ecf1b00ef7ce999a985d24ab8c7cc`) | Baseline lag regression fails `InvalidDataset`; prior full run has 14 forge, 34 HTTP and 37 overlapping review cases plus doc-test. Six focused lag cases including two new transitions and strict Clippy pass. Existing sixteen-read window, deadline and effect/identity fences remain. A later live pass is operator-reported below; its primary evidence was not independently inspected. |
| CREAO v2 #25 (`0d6d96d`) and protocol evidence #30 (`67340ba`) | 20 independent #25 tests; five CLI and nine Node #30 tests; scoped strict Clippy. Protocol-fenced routing/hash and saved evidence labels; no hosted v2 agent/run established. |

The simulator is now on main. It includes exact-shape validation, seeded families,
fixed-step physics, trace hashes, immutable hidden-set descriptors and stateless
scoring primitives. The merged acceptance adapter checks scoring grants and exact
request/execution/output identity; hidden comparison closure remains held in
#57. The source and ledger libraries cannot enforce durable exclusive state, anti-rollback, metering,
process isolation, wall/model limits or reveal timing on their own.

The merged #42/#43 foundations correct cooperative exclusive admission, false
zero-model accounting, receipt-before-durable-ledger ordering and post-seal
submission. Model use is now Unknown in every sandbox mode. Best-effort mount
restrictions and trusted-directory storage do not establish safe general
untrusted execution; no Linux namespace enforcement is independently proved here.

Process runtime/CLI and acceptance scorer/verdict are now merged: #44 source
`3040ffaab2bc59aee02c46a08b4659f9a4d864f0` →
`0604664b33c6c1439e64075b140eeb5cb1431038`; #45 source
`2a83d779d9ee49ff9374c1fd61c7f2569c641e0a` →
`07756f3e1b68be827a9f1c72f4f54eb0beb8f8f6`; #46 source
`3fcbdf74e5b4c939e80b95ef8f1310895f0cf61c` →
`2deac69e0aa1d82bc986645e0a7a33b040a4e189`; #48 source
`25b4e7419b612da9144c8e2b6f7ae74c070ddfb2` →
`194c3ff496631a40e77316779db2d4a286f77df7`. Merged tree
`3cf1b091c7eb22b43c360ff2332725aea735cda3` matches independently reviewed
composition `3f8658c05bb50f6ca54531915849d84ae371d7b8`, without #57 refactors.
Executor tests exercise 19 portable cases; one namespace function self-skips
(reported harness total 20). Scoring has 16 passes and one ignored withdrawn
policy probe. Original `8163e3f` review files are byte-identical, both request
and reply pipe probes pass, and strict all-target Clippy passes for both
standalone crates. These checks clear the earlier request-queue and verdict
precedence holds, not Linux isolation or method-loop gates.

Confirmed failed execution with independently scoreable output remains allowed.
Scored output with measured overrun remains valid retained evidence that fails
budget validation; it need not be rewritten as Failed. Scoring wall checks occur
between episodes and do not provide hard per-episode preemption. Roster and
adoption changes do not alter these boundaries.

Hidden scoring #57 `52456e363272ba554eb8b7fd76a0d5ad724ef866` now passes the
original portable recovery (`d7543ed`) and descendant (`1b61ffb`) probes:
`Mode::None` no longer asserts
whole-tree termination. Isolation #65 `64ea32130d9bb68b32505ac7cadbcfd03c40a23d`
clears the specific Linux-only-import compilation failure. Both remain unmerged.
Namespace lifecycle validation was interrupted by an automated safety filter;
no Linux validation or Linux failure reproduction was completed. The outstanding
source-level concern is whether reaping the spawned wrapper establishes that the
namespace's first process and complete tree ended on normal and forced-stop
paths. Do not use the portable results as hidden-scoring closure or hard wall-limit
evidence. See the [current coordination checkpoint](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6383718962).

Later heads #57 `81722b5abff4d9dd5fcbf5e303a9d4d505bf64b1` and #65
`635fd3b5178b6cdf0adff92c7ff21a09ebfd638a` remain unreviewed here. Henry
[reports a deadline-lifecycle fix and 24/24 Linux executor tests](https://github.com/HenryWashuHe/genesis-history-experiment/pull/65#issuecomment-6386513037).
That is author-reported evidence; independent review and the holds remain open.

Method-adoption #64 source `45938be45ddd47a4b1e4d713c4f9f05e50464089` merged
`181b81644eb2c795f0cc154501626d102219f926`; retained-file cost rehearsal #70
source `285472e6ac06d29c7866357de644ba0531600666` merged
`85616404dcd2f6de27e61d9750da0beefe401e3f`; exact-parent revision rehearsal #72
source `e1ccf65099db782e0a299f9e70540e931947cf07` merged
`751b0c6eeec1ebdaa0bb2adfaa6cb66997859e65`. Independent composition
`f527e69c21685e1a03764828e4f3fef43a398f82`, tree
`1855f3a8db02104736bc19e54e3961b95aa2ac26`, matched main after #72.
Both rehearsals and all seven adoption cases pass, with no ignored tests.
The rehearsals return **Rejected: candidate 0/3 versus control 0/3**. Tests
exercise exact parent/child and receiver/offer binding, swapped-package refusal,
correct generation-cost links, retained-file CLI equality and missing-scoring
Unknown accounting. They use handwritten sources, loopback transport and local
`Mode::None` processes. Installation records still depend on caller-retained
supporting receipts; they are not self-contained acceptance proofs. This is
fixture integration, not model generation, a completed A → B → A return or
measured recursive improvement.

Earlier #61 at `c7027a4` passed its then-current 1/1 rehearsal in 4.97 s;
#70/#72 and the composition above supersede its integration status. Its in-memory
intent roundtrip remains distinct from the separate durable-before-POST CLI tests.
All test counts are source-specific and overlapping, not one aggregate main run.

Earlier executor runs use handwritten controls. A live generated package is now
retained with AST-only inspection in the original bundle. An independently
accepted A → B → A return and controlled benefit remain unproved.
Receiving-side evidence [#98](https://github.com/HenryWashuHe/genesis-history-experiment/pull/98)
merged at `f45cb1deb57887219622b8dc123928ff764bcd9b`, source
`cb29a9593c93bbf6fe017e7e06fe7eb450d551d3`. Read-only independent checks cover
248 manifest entries, 209 artifacts, five slot bindings and the byte-identical
8,074-byte cost report. Both retained verdicts stand: **Accepted: 1/6 versus
0/6**, and separate **Rejected: 1/6 versus 2/6** against random search, using the
exact generated package. This review did not execute source, run the scorer or
call providers. The report covers the first trial only, excluding the two failed
generation attempts, separate reference run and second comparison. It is not
whole-experiment cost evidence. That bundle establishes no local adoption/promotion,
distinct B-task execution or fresh A return. Public feedback #102 source
`c8bf7a0e397d0a641239585ce5e50dcc292abd91` merged at
`7accefd3cf5da521208f17fa1474001e92aa3753`. Both prior holds are cleared:
Development-only policy is checked before record reads, and the document is
bounded to 128 KiB with explicit design omissions and listed/total counts.
Five author tests plus the three unchanged `8ad9d645` cases independently pass,
alongside strict Clippy/fmt. Read-only regeneration on copies of #98 records
produced 18,152 bytes, all 24 experiments fully listed; 55 inputs stayed
byte-identical. Hash `9218988fed77dba3ffc04ca583560f339672079c5ec8237bf61bc34ed4631472`
is local content identity, not new execution or adoption evidence.
B's [exploratory source revision](invention-lab/evidence/live-method-20261024-b-revision/README.md)
is now Generated. #112 preparation `163e952` merged before the single guarded
request; #113 source `e5d7ece` merged at `4bcd5c5`. Exact parent `8215c367` and
checked feedback `9218988f` yielded child `4f455dc6`: 6044 source bytes / 192
lines, `source_changed: true`, AST parsing only. The operator assigned B the
distinct rough-transport task; the parent's rejected random-search comparison
remains negative context. The generation bundle alone establishes no execution
or adoption; the separate receiving records below now cover B trials. Henry
owns receiving decisions and further trials with separate budgets.
Usage: 1 Anthropic `claude-sonnet-5-5` call, 20,798 input / 5,741 output tokens,
40,223 ms; one reservation remains spent. Four observed calls total 32,825 input /
24,056 output tokens and 181,342 ms summed request time. This is arithmetic,
not a prior combined grant or exhaustive billing, and excludes receiver costs.

B receiving [#116 evidence](invention-lab/evidence/live-method-20261024-b-revision-receiver/README.md),
source `59cbf0ad61b333e687f7dc8a6babdc978f7b6f31`, merged at `e091ce0d` after
independent checks: 245 manifest entries, 198 artifacts, six complete slot joins
and 12 usage/budget records; all 247 files unchanged. Child 3/6 versus random
4/6 and parent 6/6 are Rejected; versus fixed cart 2/6 is Accepted. The primary
8,087-byte cost report is exact: 1 call, 20,798 input / 5,741 output tokens,
140,640 ms and 123,113 steps, excluding other trials and earlier generations.
Checked public feedback has 32 rows/27,928 bytes (`f9759134`); four progress
floats re-encode one ULP differently, so it is not a byte-exact regeneration.
Read-only review establishes neither new execution assurance, adoption nor
improvement. Henry's [predeclared A return](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6388057984)
now has checked [#117 records](invention-lab/evidence/live-method-20261024-a-return/README.md):
source `3c1994f195f290827690a69a1d3dfcae26ae5e4e` merged at `8eed5043`.
Independent checks cover 324 manifest entries, 272 artifacts, six full joins and
12 known usage/budget records; 326 files unchanged. All verdicts are Rejected:
child 0/6 versus random 3/6, child 0/6 versus parent 1/6, parent 1/6 versus
random 3/6. Feedback contains 32/32/18 public rows; six floats re-encode within
one ULP. Exact cost reports (8,084/8,077 bytes) cover separate child/random and
parent/random trials: respectively 1 call, 20,798/5,741 tokens, 101,151 ms /
75,636 steps; and 1 call, 4,028/5,231 tokens, 76,265 ms / 44,140 steps. They
exclude child/parent comparison and earlier attempts/trials; do not sum them
with other reports as exhaustive history. Organization IDs are labels on one
operator's host, not independently authenticated organizations. No local
adoption, accepted return or positive benefit is established; G0–G4 stay open.
[Explicit reasoning policy #120](invention-lab/contracts/generation.md#settings-and-accounting)
is now merged: Sonnet 5.5 adaptive effort is part of frozen client settings.
It does not pin hosted weights/stochasticity or upgrade prior runs to G4. No live
request or new generation is declared. The pure retained-reference catalogue
audit #124 is merged; Henry retains any future pre-experiment enforcement.
Whole-chain cost assembly #127 and its [read-only CLI #130](invention-lab/contracts/chain-allocation.md#read-only-chain-cost-cli)
are merged, preserving unknowns and local overruns without counting nested totals
twice. These are accounting mechanisms, not a new experiment result.
[Method registry API #132](https://github.com/HenryWashuHe/genesis-history-experiment/pull/132)
is merged: authenticated publisher, bounded source-free keyword metadata search
and full-reference fetch in the existing process. [Typed client #133](invention-lab/contracts/method-registry.md#typed-organization-client)
is also merged; the local fixture reaches B revision preparation only. The
[operator CLI #141](invention-lab/contracts/method-registry.md#operator-cli) now exposes
explicit publication/search/selected fetch without automatic `.env` loading,
journaling or retry. Any publication error may follow a committed write. Source
transport grants no acceptance or invocation authority. #150/#151 now carry the
explicit selection and exact parent into a bounded revision bundle and ordinary
Prepared checkpoint. Henry retains receiver adoption and role/grant work.
Henry [acknowledged the vertical product slice](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6388913830):
recompute retained acceptance, bind the selected method to requirement/responsibility
and current owner grant, then check pinned invocation, decline/revocation and an
existing-owner control. This server connection is not implemented; Henry keeps
ownership, with [pure core verdict #146 now merged](https://github.com/HenryWashuHe/genesis-history-experiment/pull/146)
as a prerequisite. Execution under the grant and justified specialist creation
remain later completion work. The W/F pilot alone cannot close organizational evolution.
A fixed-pool search-policy ablation remains a separate proposed mechanism study,
with task-artifact and identical-information local controls; it is not full G4
or a restriction on capability development. Henry is preparing a corrected K=3
workflow-versus-fresh declaration across all four families, adaptive/high client
effort, neither arm seeded with old feedback, and public fresh-task assessment.
[Protocol #135 `e5f7c326` is held](https://github.com/HenryWashuHe/genesis-history-experiment/pull/135#issuecomment-6389154691)
on conflicting Failed+Scored eligibility and an unspecified Interrupted rule;
prior adaptive-preparation, missing-pair and random-reference holds are addressed.
Public-range #121/#136 is merged with the reserved-seed
hold cleared; metadata/preflight and mocked checks do not hide inputs or establish
an evaluation result. Freeze
source/selection before assessment, retain failures and actual costs, and agree
exact protocol identities/whole-chain ceilings before new generation.
Predeclare all six operation/stage slots and full ceilings before work. W rounds
2/3 acquire exact Prepared request hashes only after their declared feedback
exists; future prompt hashes must not be fabricated.

Eric retains the UI lane. #93/#94/#99/#100 corrections are merged: label
fallback, organization binding, exact ledger arithmetic and execution/scoring
attribution. #94 `7ed70f9` composition `ef484cbf` passed 21 focused checks/
typecheck; all six original cases and the full 121-line `78adef37` file remain
unchanged on main, including #115's retained label cases. #99 `f09bf473` /
#100 `76c2cd54` passed 42 focused checks/typecheck, including four unchanged
regressions, four actual Rust receipt hashes and 11 SHA vectors. #101 `ed8b64de`
merged at `95256998`: 52 focused UI checks/typecheck pass, preserving the full
119-line `59c3e1c` file. #109 `2cfb4058` merged at `5ed65deb`, passing 66
focused tests/11 files and typecheck with all five original `f2ac45e5` cases
unchanged. Real-evidence #123 `7196d5c` passed 81 tests across 13 files/typecheck;
original `0672dcbe` regressions are unchanged and pass. Merge `4515f728` went to
its parent branch only; #128 `2f0b6734` subsequently integrated identical web
bytes into main at `a3be74d0`, with all 81 tests/typecheck independently rerun.
This clears the earlier folder-dedup, raw-package and inspection-identity holds.
In the separate `apps/stage` queue, #125 `973aa9c` remains held on organization-timing attribution; stacked
#126 `b7bdb18` is held on four mocked acknowledgment failures: early-decision
race, poll-failure evidence loss, cross-organization binding and malformed success.
Original `d85b018` regressions are retained. G2/G3 organizational authority, selection
and local adoption are separate from any displayed story or receipt identity.
Henry's receiving wrapper #118 `68bc82d0` merged at `ec23efb5`: copies checked
package/spec bytes and atomically records each completed/failed launch. All six
original mocked cases pass with the full 203-line `e3f3779` unchanged; reviewed
composition `428c1ceb` / tree `a996da20` preserves source bytes. Trusted-host
storage only; no generated-source/runtime/namespace/provider execution in review.
Archival stage presenter #108 `ece3c614` merged at `d810a670`: reviewed
composition `1632f08` / tree `0ab8d80a` passes 22 Node checks and independent
DOM probes. Visibility/stale-caption findings cleared; presentation support is
separate from goal progress and revives no event-organizer direction. Eric's [separate CREAO ticket-reconciliation claim](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6387513389)
merged as #104 (`5023a48` → `62a2f009`), with 69 original tests and three
independent guards retained in #106 (`e8cf6010` → `90a26e00`). This establishes
local recovery mechanics, not hosted recovery or readiness; the old unretained
run ID cannot be recovered retroactively. ACP keeps bridge/docs and hosted-gate
evidence. Ines stays unassigned. See the [current direction checkpoint](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6387556484).

ACP #3 source `f64e131516281d6bb1dc5f41abd8425cc42c77d1` merged
`5b2615f0918f0e96d9b6f94135737ab1e8b2e98c`; #5 source
`e95a2862e055eaba39ca040737093274fc25cc51` merged
`4ac3620a6d65423fc6a4b9922cd74b21156b24d4`. Independent bridge suites pass
148/148 and 158/158, zero skips. All eight cases in the `bb5a841`, `0705ecd`
and `b60a755` regression files are unchanged and pass. The correction removes
the old named indexes inside the rebuild transaction, preserving rollback while
allowing intermediate schemas to reopen. This clears the prior atomicity and
index-name collision holds; the earlier 145/155 passing baselines had not covered
that failing intermediate-schema case.
Reviewed #3/#5 composition `9c636f198be271490847a65421385dc7f4e56e43`, tree
`5d623f4ef694190c53cf5bd06ab8000d710660e3`, has the same bridge subtree as final
main: `5d040d9c7201819a5f7f477646e9d82c3906e6fa`. See [the clearance](https://github.com/HenryWashuHe/genesis-history-experiment/pull/3#pullrequestreview-5638260885).
Evidence remains SQLite/loopback/signed-process testing, not public Agentverse/ASI
routing. ACP retains #4, whose stale routing/status/demo references must be
reconciled with merged #3/#5, the local executor/scorer and issue #1. Do not revive
the event-organizer example. #6 is withdrawn and the venue fallback #2 stays closed.

The [latest Apify operator report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6382245294)
records a passed live gate on main `2a070af`: three oracle-checked cases with the
handwritten baseline parser, Actor `n8kB1hNqgzkddvWKp`, build `D5hU7kSdSc20L08g3`.
Sanitized evidence files and the private checkpoint were unavailable for
independent inspection here, so this remains **operator-reported live evidence**.
It is a new operation after #47, not a retroactive pass of the earlier failed
checkpoint. It establishes no model generation or development-method result.
CREAO hosted execution, Fetch public use and Elastic permissions retain their
separate [live gates](LIVE_GATES.md).

Codex's merged [method-generation path](invention-lab/contracts/generation.md)
is available for scoped integration with Henry's runtime. It saves the complete
historical request, reconciles an interrupted request before reading credentials
and never automatically resubmits an uncertain attempt. Its current
`provider_default` sampling policy is not a fixed-settings G4 result. Generation
costs can now be combined by the merged [declared-roster accounting library](invention-lab/contracts/accounting.md),
including failed/missing generation with no trial slot. The host still declares
history and inherited-cost inclusion; the library does not establish roster
completeness, budget admission/enforcement, authenticated metering or an acceptance
verdict. Provider/settings reproducibility and the controlled comparison remain open. The merged
[offline read-only CLI](invention-lab/contracts/accounting-cli.md) exposes that
assessment without checkpoint mutation or provider calls. Omitted declared
evidence stays unknown; a named missing/malformed/substituted file is an error.
Exit 0 means assessment completed and can include unknown usage or overruns,
not that a method was accepted or its budget was satisfied.

The merged [revision preparation API](invention-lab/contracts/revision.md)
adds a G3 prerequisite: canonical parent bytes become an exact frozen public
input to one ordinary generation attempt. A read-only observation validates the
whole saved plan and distinguishes source byte changes from metadata relabeling.
It does not authenticate org identity, prove parent adoption or show improved
behavior. Its tests construct synthetic completions without HTTP. The merged
[offline CLI](invention-lab/contracts/revision.md#cli) prepares the existing
checkpoint exclusively and observes exact source derivation without mutation.
These commands make no provider request and execute no source.

The merged [generation roster](invention-lab/contracts/generation-roster.md)
freezes 1–64 Prepared attempts, their order, common host-selected settings and a
generation-only grant before observing results. It retains source-free plan,
request and initial-checkpoint identities. Later snapshots must match those
identities, including the saved historical request; foreign/duplicate/substituted
snapshots fail. Every declared missing, unused, failed, rejected or uncertain
attempt remains in the report. Initial-identity derivation is read-only and does
not reset or replay a session. Declared request/output-cap feasibility does not
enforce provider spend, input-token/wall limits, authenticated chronology,
exhaustive history, fixed provider behavior or benefit. Execution/scoring budgets
remain separate, and an adaptive chain still needs an allocation policy.
The merged [offline CLI](invention-lab/contracts/generation-roster.md#offline-cli)
provides exclusive `method-roster-prepare` and read-only `method-roster-assess`.
Omitted declared observations remain Unknown; named missing/malformed files are
errors. Exit 0 means a report was constructed, including possible unknown usage
or overruns. No provider request, recovery or source execution occurs. #75 retains
the two independent historical-identity and CLI-boundary tests unchanged.

The merged [generation reservation library](invention-lab/contracts/generation-admission.md)
adds a source-free journal of exact Prepared identities reserved once against
their original roster. Its request-count and requested maximum-output caps apply
to one trusted journal. Reservations never refund, including stranded, failed or
unknown attempts. Actual/unknown usage remains separately assessed. Copies,
rollback and ungated commands can bypass this cooperative boundary; it does not
enforce input tokens, aggregate wall time, provider billing or global spend,
authenticate chronology, prove complete history or close G4.
The merged optional `method-generate-step --admission` holds the admission lock
before the generation lock and saves the reservation before Requesting intent
and the existing one-attempt provider path. Fresh invalid configuration consumes
no reservation. A stranded Prepared reservation cannot submit; a reserved pending
attempt reconciles without replay/refund, and an advanced unreserved checkpoint
is refused. Offline prepare refuses overwrite; status is read-only. Legacy steps
without the option remain outside these bounds. Validation uses offline/loopback
fixtures, not live provider enforcement.

The merged [admission-aware cost join](invention-lab/contracts/admitted-costs.md)
requires the entire original roster in its declared order and checks each later
snapshot against its historical Prepared identity. Advanced attempts need a
reservation; only validated current checkpoint hashes are projected into the
existing trial/cost assessor. The report binds the original and projected scopes,
journal and roster while retaining separate generation and combined grants.
Missing, reserved-but-Prepared, failed, rejected, unused and unknown attempts
cannot disappear. Known lower bounds, overflow and overruns remain visible.
One roster joins one optional trial; an exact method can be reused across slots
without counting its generation twice. This does not establish exhaustive
accounting across separate adaptive stages or a scope allocated before spending.
The optional `method-cost-assess --admission` is read-only and fails malformed,
missing or substituted named inputs without falling back to the legacy report.
It does not lock, recover, reserve, refund or execute work. Retain immutable
snapshots: reading mutable files does not provide an atomic multi-writer view.

The merged [stage allocation and CLI](invention-lab/contracts/stage-allocation.md)
add a structural resource envelope: full generation grant plus the maximum slot
count times development/scoring grants must fit every stage ceiling, with checked
arithmetic. Preparation locks the admission journal and requires the full unused
Prepared roster before creating the allocation. Later binding pins the exact
trial and original cost scope in one create-only companion per canonical path.
Optional `--allocation` validates generation context before recovery/credentials/
reservation and binds read-only cost assessment to that companion. Unknown usage,
overruns and original generation-grant limits remain visible. Runtime entry checks
are still Henry's work; copies, rollback, stale snapshots and chronology remain
outside this cooperating-path mechanism.

The merged [finite-chain contract](invention-lab/contracts/chain-allocation.md)
reserves the full ceilings of 1–64 ordered generation or trial-only stages in all
five resource dimensions. Later bindings check exact settings, organization,
grants and trial/scope identities without inventing generation for inherited,
control or return work. Declined/unbound stages retain their full capacity.
This is a pure structural library: host retention before work, one authoritative
binding per stage, runtime consumption, chronology, exhaustive accounting and
actual enforcement remain outside it. It cannot retroactively allocate the three
observed live calls or close G4.


The replacement project credential now works with `anthropic` / `claude-sonnet-5-5`;
both model-catalog authentication checks returned 200. The [first live attempt](invention-lab/evidence/live-method-20261024/README.md)
failed protocol validation: no package or source execution, with one reservation
retained and no replay. Usage was 1 call, 3,971 input / 8,192 output tokens and
62,131 ms. Reaching the output cap does not prove truncation: the stop reason was
not retained. Independent retained-file consistency checks are not cloud-source
authentication. [Closed diagnostics](invention-lab/contracts/generation.md) are now
implemented at source `f4fa271e603d3b38be1b8d9caeb2215bda2f04fd`: 206 forge tests
across 38 targets pass, zero failed/ignored; strict all-target Clippy/fmt/diff pass.
Those local checks preserve #89 unchanged. The [separate second cloud attempt](invention-lab/evidence/live-method-20261024-diagnostic/README.md)
on `7d6119d` failed with `content_shape` / `end_turn`: 1 call, 4,028 input / 4,892
output tokens, 36,790 ms; no package/execution and no reservation refund.
The [strict thinking-prefix parser](invention-lab/contracts/generation.md) is implemented
at `054b10508713095ee3bbac97eb1d9d953956596b`: full forge 211 tests/40 targets, zero
failed/ignored; strict all-target Clippy/fmt/diff and five independent focused tests pass.
This is local compatibility evidence; neither old failure is reinterpreted. The
[third attempt on `07c098c`](invention-lab/evidence/live-method-20261024-generated/README.md)
produced a canonical package: 4027 source bytes/137 lines, AST parse only. Usage:
1 call, 4,028 input / 5,231 output tokens, 42,198 ms. Henry owns its exact-bound
runtime/acceptance handoff; no runtime retagging. Separate #98 receiving
execution/scoring records are checked above; adoption and the return remain
unproved. G0–G4 remain open; chain ceilings and retained CLI are merged.
Earlier delivery/source-specific evidence remains in the handoff; overlapping
test counts must not be summed. Repair WIP is isolated in draft #7 and its
original files remain untouched. Follow current
[ownership](AGENT_HANDOFF.md#current-ownership-and-next-work) and the
[goal ledger](ONGOING_GOAL.md#current-milestone-ledger) for the next step.

## Historical source checkpoint before migration


This checkpoint describes main `2e5d7f07784eff8563171fd34c85e79584b59fe7`
and separately identified work as of October 23, 2026 (Pacific time). Newer exact
heads and [issue #4](https://github.com/HenryWashuHe/genesis/issues/4) supersede
it. The [review queue](PR_REVIEW_QUEUE.md) records unresolved findings; the
[handoff](AGENT_HANDOFF.md) records ownership and authorization to continue.

GENESIS's direction is **networked recursive capability improvement with fixed
model weights**: organizations develop executable capabilities, selectively
inherit them, and improve the methods used to develop future capabilities.
Retaining a tool, transferring it, and improving the development process are
distinct claims. The [invention-lab demo](INVENTION_LAB_DEMO.md) is a proposed
way to test the larger claim; it is not an implemented simulator or a measured
result. Event organizing is no longer the product direction. Existing venue
fixtures remain regression evidence for the mechanisms they exercise.

## Merged implementation and its evidence

| Area | Present on the inspected main | Evidence boundary |
| --- | --- | --- |
| Organizational authority and persistence | Committed roles, responsibilities, assignment epochs, leases, capability attachment, retirement, event history and durable effect journals. See [core engine](../crates/genesis-core/src/engine.rs), [store](../crates/genesis-server/src/store.rs) and [saga](../crates/genesis-server/src/saga.rs). | Local invariant, integration and fault-injection checks exercise specific boundaries. Dynamic organization structure alone does not demonstrate better problem solving. |
| Generated task capabilities | Requirement/public-example synthesis, exact source/build binding, independent trusted-suite verification and one bounded repair of a rejected initial acquisition. See [forge acquisition](../crates/genesis-forge/src/acquisition.rs) and [server acquisition runtime](../crates/genesis-server/src/capability_acquisition_runtime.rs). | The generated runtime is specialized to TypeScript web extraction on Apify. Loopback clients/journals are exercised; canned model/provider output does not demonstrate that generated code works in a live provider. |
| Publication and independent inheritance | Registry publication/discovery, exact version/pin checks, receiving-organization acceptance, and retained capability use after the original specialist retires. See [registry](../crates/genesis-server/src/registry.rs), [adoption](../crates/genesis-server/src/capability_adoption.rs) and [two-organization tests](../crates/genesis-server/tests/two_org_inheritance.rs). | Fresh local checks in B are implemented. These fixtures establish bounded inheritance mechanics, not general applicability, measured improvement, authenticated shared repair advisories or automatic migration of installed versions. |
| Invocation evidence | Backend reconstruction of collected invocation evidence against exact artifact, input, suite and historical authority. See [invocation verification](../crates/genesis-server/src/capability_invocation.rs). | Semantic rejection is distinct from authority rejection and uncertain provider effects. The accepted suite defines the supported input scope. |
| Fetch transport and automatic recovery intake | Durable specialist contracts/messages, exact task receipts, verified recovery coordination and opt-in intake. See [Fetch crate](../crates/genesis-fetch/src/lib.rs), [intake](../crates/genesis-server/src/venue_recovery_intake.rs) and [native composition](../crates/genesis-server/tests/venue_assessments/recovery/native.rs). | #117's two explicit native tests use actual Python SDK processes and signed local transport. Apify, CREAO and initial provisioning remain fixtures. Intake is explicitly drained in the test; timer scheduling and public Agentverse/ASI execution are not established by it. |
| Operator console and ACP entry points | Organization/audit views and fixed recovery/mission adapters exist. See [web application](../apps/web/src), [recovery adapter](../services/fetch-bridge/genesis_fetch_bridge/acp_recovery.py) and [mission adapter](../services/fetch-bridge/genesis_fetch_bridge/acp_mission.py). | The mission ACP adapter merged with unresolved independent regressions. It is not a validated public end-to-end workflow. Invention-lab method lineage and comparative-evaluation panels are proposed. |

The merged scenarios use real server state transitions, adapters and verification
code with explicitly bounded fixture inputs. Some transport checks also use real
local SDK processes. Neither category establishes authenticated live cloud
execution, a complete autonomous mission, physical-world engineering results or
measured recursive improvement. Source-specific test counts in the review ledger
must not be summed into a claimed test run of current main.

## Paused, unmerged deployed repair

The `codex/deployed-capability-repair` branch preserves forge commit `1795fb7`
(the scoped source commit is `be2479a`). It adds a deployed-repair constructor
bound to the parent's current independently verified parser, an immutable origin
digest, one fresh replacement and unchanged-parser rejection. Its 19 acquisition
tests passed before the pause. This code is not on the inspected main.

Server repair creation/store/routes, 12 integration cases and draft repair docs
remain local uncommitted work. The complete failure → replacement → publication →
independent B acceptance happy path passed. The full revised 12-case target and
final checks had not run when work paused. No repair PR was opened or merged.
See the [preserved checkpoint and pause notice](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6378835715).

That slice covers semantic failure on an already authorized frozen input. It
does not authorize unseen source layouts, revise historical acceptance evidence,
or implement cross-organization repair advisories. Preserving the work does not
make its old domain a requirement for the new invention-lab direction.

## Open review and known limitations

| Work | Inspected status |
| --- | --- |
| [#116 candidate decisions](https://github.com/HenryWashuHe/genesis/pull/116), correction `78a18c059e13dde3218da68615928bad05291b6f` | Correction pushed; independent re-review pending. The author reports all three prior P2 findings fixed, `4b44099` preserved, and 46 gap / 8 adoption / 5 scenario / 1 inheritance checks passing. The selection-order, unreadable-score and missing-conflict-log findings apply to reviewed `03f7101f`; they are not established findings against the new head. |
| [#120 constitution audit](https://github.com/HenryWashuHe/genesis/pull/120), `0cf383b7` | Open; root review/validation pending. Author-reported checks do not constitute an independent merge-ready decision. Eric retains its bounded audit scope. |
| [#121 terminal RunFailed](https://github.com/HenryWashuHe/genesis/pull/121), `5d711429` | Open; root review/validation pending. Proposed distinction between confirmed terminal provider failure and uncertain/live effects is not yet merged. Henry's core Claude retains this narrow provider/runtime interface change. |
| Merged [#114 mission ACP](https://github.com/HenryWashuHe/genesis/pull/114) | Seven independent regressions remain outstanding, including duplicate submission after restart, false no-change status and incomplete/misattributed follow-up outcomes. See [the current-main reproduction](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6378650906). The ACP author/session remains separate from Henry's core adoption work. |

Henry's core Claude has also [claimed the next adoption outcome classifier](https://github.com/HenryWashuHe/genesis/issues/4#issuecomment-6379056143),
stacked on #116: `adopted`, `not_applicable`, `insufficient_evidence`,
`no_candidate`, `unknown`, plus pending/stopped states, with types/status,
focused classifier tests and docs. This is an active bounded claim, not a merged
interface. The conservative fallback contract remains unchanged. Coordinate
new method-adoption contracts with this owner rather than duplicating that work.

## Proposed, not implemented or demonstrated

- A generic executable development-method package and executor, with exact
  dependencies, bounded effects and independent acceptance. Workflow text in a
  manifest is not an executable development method.
- A visual physics invention lab, deterministic domain evaluators, independently
  controlled challenges and a working reciprocal A → B → A method-improvement
  demonstration. These require contracts beyond the current extraction runtime.
- Measured benefit from development-method inheritance over a strong equal-budget
  baseline, including failures, regressions, inappropriate transfer and the cost
  of generating and validating the shared method. A passing suite or growing
  artifact count is insufficient evidence.
- Authenticated network repair advisories, general method lineage, deprecation,
  rollback policy and independently accepted replacement selection across
  organizations. Existing installation pins must remain explicit.
- Public Agentverse/ASI operation and authenticated live sponsor execution that
  completes the intended workflow. No established full-mission evidence is
  claimed here.

The [recursive roadmap](RECURSIVE_CAPABILITY_IMPROVEMENT.md) and
[invention-lab proposal](INVENTION_LAB_DEMO.md) describe intended next work.
Ongoing work is resumed under the [repository goal](ONGOING_GOAL.md) and active
hourly continuation. The old app Goal card still needs lifecycle replacement;
that does not revive the venue objective or transfer colleague files. Local
deployed-repair WIP remains separately preserved. Ines is explicitly unassigned.
Consult the [current handoff](AGENT_HANDOFF.md) before taking any files, including
new contracts or simulator paths.
