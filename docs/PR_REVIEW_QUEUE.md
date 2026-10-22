# PR review queue — October 22, 2026

The user authorized merging ready PRs on October 22, 2026. All eleven capability
PRs below are now merged to `main`; eleven PRs remain open. Fresh read-only audits
confirmed the recorded heads, no new actionable comments, intact dependencies
and exclusion of held source changes. Every merge used the reviewed head, a merge
commit and a tree-equality check; branches were retained and children retargeted
to `main` sequentially. No administrator bypass or squash/rebase was used.

Main's capability checkpoint is `4ecc3970634a04b54e7e7dbbe952fbc4e4f980cb`.
Its tree is exactly the reviewed/tested #24 head `9e7939a` (594 Rust tests,
formatting and strict Clippy). The unchanged suite was not repeated for the
history-only merges. Documentation updates do not introduce sponsor source code.

## Capability stack: merged to main

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

The stacked head has prior local evidence of 594 passing Rust tests across 64
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

## Claude/Eric source fixes: hold these PRs

The original #11/#12 regressions pass on the updated heads. The follow-up findings
below remain open. Keep fixes on their source branches so other machines can
consume them without duplicating implementation in the integration branch.

| PR / reviewed head | Required correction | Review evidence |
| --- | --- | --- |
| [#11](https://github.com/HenryWashuHe/genesis/pull/11) / `aa94d1006e6bad4833f5a9aef7989e4c1e763790` | Check mission deadline and assignment lease before/after Elastic calls; prevent generic adapter results from finalizing backend-managed jobs | [Review](https://github.com/HenryWashuHe/genesis/pull/11#pullrequestreview-5615583092), five reproduced failures in test-only `b2037e5` |
| [#12](https://github.com/HenryWashuHe/genesis/pull/12) / `22fefe62a91bae948179267bd9a13cd9943b6482` | Persist requirement-create uncertainty/identity across restart; handle reconciled or cached REJECTED changes as rejection | [Review](https://github.com/HenryWashuHe/genesis/pull/12#pullrequestreview-5614141053), three reproduced failures in test-only `70c4edb` |
| [#20](https://github.com/HenryWashuHe/genesis/pull/20) / `e8cad2054e1b2961d061bed432c9389b1c45af39` | Apply the managed-job result guard to specialists; reauthorize before a retry after handoff | [Review](https://github.com/HenryWashuHe/genesis/pull/20#pullrequestreview-5614181896), three reproduced failures and a passing control in test-only `faf3d8b` |
| [#21](https://github.com/HenryWashuHe/genesis/pull/21) / `2c987193892da94e4110179362f2ef2aa052779c` | Bind acceptance runs to installation ID, support adoption acceptance purpose, and refresh changed/failed evidence reads | [Source review](https://github.com/HenryWashuHe/genesis/pull/21#pullrequestreview-5615620684); no new browser reproduction |
| [#22](https://github.com/HenryWashuHe/genesis/pull/22) / `8a23273ccbf97b611ed73ddfc43646b31dffbcca` | Revalidate effective remote planner configuration; disable redirects in the shared Agent Builder client | [Source review](https://github.com/HenryWashuHe/genesis/pull/22#pullrequestreview-5615620695); no new drift/redirect reproduction |
| [#26](https://github.com/HenryWashuHe/genesis/pull/26) / `3dc95d6bd9863c09e5775957d5346edbc3d35d2e` | Reconcile a provisional receipt run ID against the trusted dispatch result before treating correlation as verified | [Review](https://github.com/HenryWashuHe/genesis/pull/26#pullrequestreview-5616058167); test-only `8d82c32`, one reproduced failure and a passing late-receipt control |
| [#27](https://github.com/HenryWashuHe/genesis/pull/27) / `95a33a0edd1b574eef59cbe6a11b0a09915978aa` | Include sibling bindings in the current change when checking whether an attached native agent is already in use | [Review](https://github.com/HenryWashuHe/genesis/pull/27#pullrequestreview-5616058543); test-only `e161aad` reproduces two committed roles sharing the same native agent |
| [#28](https://github.com/HenryWashuHe/genesis/pull/28) / `de0f5bc03c5728286bfd921b73f892d06eb88b7f` | Keep an Abandon-only action after attachment while readiness is pending | [Review](https://github.com/HenryWashuHe/genesis/pull/28#pullrequestreview-5616058888); actual component rendering loses both actions on transition to awaiting readiness, not an interactive browser test |

Prioritize the shared backend-managed result guard across #11/#20 first, then
time-sensitive authority/retry checks and planner restart behavior. Put the shared
Agent Builder transport correction in #11 and absorb it into #22. Finish #21's
installation/adoption evidence bindings against the current capability contracts.
The new recovery stack is #20 → #26 → #27 → #28. Fix #27's sibling identity
collision before relying on operator attachment; fix #26's run correlation without
losing valid early receipts. #28 should preserve abandonment without exposing
Attach for an already-bound operation. These findings are additions to the
earlier source review, not evidence that the earlier blockers were fixed.

## Provider readiness bridge: draft and dependent on kernel fixes

[Draft #29](https://github.com/HenryWashuHe/genesis/pull/29),
`c1e6ba4427203eb30e04019a42e997cdf6d6f393`, is based on #26 at `3dc95d6`.
The separate code review checked immutable challenge binding, submission-only
concurrency, cancellation/restart, successful POST followed by a failed receipt
save, and privacy. Both requested follow-ups are fixed and tested: provider model
metadata cannot expose a nonce through Debug, and a failed completion checkpoint
stays uncertain without replay. No new blocker was found in this provider slice.
Its focused workspace passed 407 Rust tests, formatting and strict Clippy.

Keep it draft while the parent #26 correlation and #20 authority findings remain.
It does not authorize readiness from native completion or establish actual skill
installation. No live provider run was made. This is source review and loopback
validation, not a GitHub approval or evidence that the inherited defects are fixed.

## Documentation and combined checkpoint

- [#23](https://github.com/HenryWashuHe/genesis/pull/23), `665aab42177098330ac5bff71656ebf201fad5a9`:
  refresh after the source fixes. Replace the three "to open" cells with #20/#21/#22,
  qualify the #11/#12 review-fixed claims, and correct CREAO registration status:
  specialist registration exists in #20, the kernel gateway verifier is in #26,
  and the durable provider probe/registration is in draft #29. Live skill/account
  proof remains pending. Reconcile these docs with the newer integration handoff.
- [Draft #25](https://github.com/HenryWashuHe/genesis/pull/25): keep draft. Reviewed
  integration snapshot `1004934` includes #26–#29 as well as the earlier heads.
  It passes 699 Rust tests across 87 suites, strict Clippy and formatting.
  Unchanged web/schema sources retain the earlier `dde0682` validation: 33 web
  tests, production build and schema/type regeneration without drift.
  The thirteen failing source-review probes are separate and are not in that
  passing baseline. [Full validation scope](https://github.com/HenryWashuHe/genesis/blob/8a7b115/docs/COMBINED_INTEGRATION.md).

Before landing any held item, verify its findings are fixed on the exact proposed
head and rerun the affected regressions. Reconcile source branches and draft #25
against current `main`, preserving ancestry; do not merge the combined draft while
its source findings remain. Passing fixture tests do not close live-provider gates.

Codex owns capability code/review and narrow integration. Claude/Eric retains
planner, general kernel/lifecycle/saga/outbox, asynchronous role activation, Elastic
and UI. Codex completed the previously claimed provider readiness gap in #29;
the immediate priority remains verifying source fixes and draining this queue.
No whole-kernel ownership transfer is active.
