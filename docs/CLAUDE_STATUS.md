# Claude/Eric PR status — October 22, 2026

All branches below merged `main` at `e57097f` (capability stack #8–#24 and #30)
and carry the Codex review tests from the `codex/review-*` branches. Every
reproduced regression now passes. Fixes are on the source branches; no review
test was weakened, except that `review_specialist_authority.rs` gained an
`#[allow(dead_code)]` on an unused scripted step so strict Clippy passes.

| PR | Finding (review) | Fix |
| --- | --- | --- |
| [#11](https://github.com/HenryWashuHe/genesis/pull/11) Elastic | Mission deadline / assignment lease not checked around Agent Builder calls; generic results route could finalize Elastic jobs (`b2037e5`, 5 tests) | Full current grant via `capability_authority::authorize_job` before the call and before applying the answer, plus the same agent binding; `409 managed_elastic_result`; Elastic HTTP clients never follow redirects (`no_redirects.rs`) |
| [#12](https://github.com/HenryWashuHe/genesis/pull/12) Planner | Ambiguous requirement POST could duplicate after restart; cached/reconciled `REJECTED` treated as success (`70c4edb`, 3 tests) | Kernel requirement route accepts `client_request_id` (one requirement per ID, `409 request_id_conflict` on a different body); planner uses `planner:<obs>:requirement`; stored rejections enter repair; restart resumes after spent attempts |
| [#20](https://github.com/HenryWashuHe/genesis/pull/20) CREAO hook | Generic results route could finalize specialist jobs; read retry skipped reauthorization (`faf3d8b`, 3 tests) | `409 managed_specialist_result` from creation onward (durable provider key); authority before every provider call; full current grant as for Elastic |
| [#21](https://github.com/HenryWashuHe/genesis/pull/21) Capability evidence UI | Acceptance grouped by version; stale checkpoints | Acceptance bound by run purpose `installation_id` (incl. `adoption_acceptance`); other runs listed separately; active/retryable/failed reads refreshed every 5 s |
| [#22](https://github.com/HenryWashuHe/genesis/pull/22) Elastic planner model | Cached agent not revalidated; redirects in shared client | Exact configuration re-read before every conversation (drift test); redirect fix from #11 |
| [#26](https://github.com/HenryWashuHe/genesis/pull/26) Gateway readiness | Early receipt with a wrong run ID became Ready (`8d82c32`) | Receipt run kept separately and must match the dispatch's trusted run; mismatch voids the receipt |
| [#27](https://github.com/HenryWashuHe/genesis/pull/27) Operator attach | Sibling roles of one change could attach the same agent (`e161aad`) | Sibling bindings checked under the kernel lock |
| [#28](https://github.com/HenryWashuHe/genesis/pull/28) Attention panel | No Abandon after attach | Abandon-only action for awaiting-readiness and retrying operations (render test) |

Merge order: #11 → #12 → #22; #20 → #26 → #27 → #28; #21 any time; this
documentation PR (#23) last. Stacked PRs need retargeting to `main` after their
base merges. No live Elastic, CREAO or model provider was contacted; all
provider behavior in these tests is a fake, scripted runtime or loopback
fixture. Fresh immutable invocation proof display in the console remains a
pending follow-up.
