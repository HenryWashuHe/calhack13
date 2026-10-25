# Working repository and migration checkpoint

The canonical public repository is now [calhack13](https://github.com/HenryWashuHe/calhack13).
On Friday, October 23, 2026 (Pacific), active work moved from `genesis` to
`genesis-history-experiment`; that destination was subsequently renamed to
`calhack13`. The rename preserves issue/PR history and existing links redirect.
Use [coordination issue #1](https://github.com/HenryWashuHe/calhack13/issues/1),
[current ownership](AGENT_HANDOFF.md#current-ownership-and-next-work), and the
[Friday-start timeline](PROJECT_TIMELINE.md). Former `genesis` PR numbers still
need the mapping below; a rename does not merge the two original histories.

The existing primary local checkout is `/Users/apple/Documents/ChatGPT/genesis-history-experiment`;
its directory name need not match the canonical remote. Automation and the native
Goal remain paused. [ONGOING_GOAL.md](ONGOING_GOAL.md) retains the completion
contract; documentation maintenance does not resume experiments or automation.
The lower migration, credential and preserved-work records are dated history.

## Current PR routing

| Former repository PR | Active repository PR |
| --- | --- |
| [116](https://github.com/HenryWashuHe/genesis/pull/116) | [8](https://github.com/HenryWashuHe/genesis-history-experiment/pull/8) |
| [120](https://github.com/HenryWashuHe/genesis/pull/120) | [9](https://github.com/HenryWashuHe/genesis-history-experiment/pull/9) |
| [121](https://github.com/HenryWashuHe/genesis/pull/121) | [10](https://github.com/HenryWashuHe/genesis-history-experiment/pull/10) |
| [128](https://github.com/HenryWashuHe/genesis/pull/128) | [11](https://github.com/HenryWashuHe/genesis-history-experiment/pull/11) |
| [129](https://github.com/HenryWashuHe/genesis/pull/129) | [14](https://github.com/HenryWashuHe/genesis-history-experiment/pull/14) |
| [130](https://github.com/HenryWashuHe/genesis/pull/130) | [2](https://github.com/HenryWashuHe/genesis-history-experiment/pull/2) |
| [132](https://github.com/HenryWashuHe/genesis/pull/132) | [3](https://github.com/HenryWashuHe/genesis-history-experiment/pull/3) |
| [133](https://github.com/HenryWashuHe/genesis/pull/133) | [4](https://github.com/HenryWashuHe/genesis-history-experiment/pull/4) |
| [134](https://github.com/HenryWashuHe/genesis/pull/134) | [12](https://github.com/HenryWashuHe/genesis-history-experiment/pull/12) |
| [135](https://github.com/HenryWashuHe/genesis/pull/135) | [5](https://github.com/HenryWashuHe/genesis-history-experiment/pull/5) |
| [136](https://github.com/HenryWashuHe/genesis/pull/136) | [13](https://github.com/HenryWashuHe/genesis-history-experiment/pull/13) |
| [137](https://github.com/HenryWashuHe/genesis/pull/137) | [15](https://github.com/HenryWashuHe/genesis-history-experiment/pull/15) |

The adoption/outcome/refusal and ACP command stacks retain their branch bases.
Migration does not make a PR merge-ready. Exact source review, independent
regressions and relevant checks still apply. New comments belong on the active
PR; retain links to original reviews and author contributions.

Additional work: [#6](https://github.com/HenryWashuHe/genesis-history-experiment/pull/6) is closed as superseded by #17: its sync script was withdrawn and the routing notice is already on main;
[#7](https://github.com/HenryWashuHe/genesis-history-experiment/pull/7) is preserved deployed-repair WIP. The helper's withdrawn script
still has unresolved review findings; do not use an old branch copy. The current manual migration uses explicit mapped bases and complete
tree differences, not patch-ID equivalence alone.

## Verified content and history boundaries

The initial main trees are identical (`8b3c1e2e288c2a8261399630616a0078c9ce9cae`):
legacy `8794e25a04f89b56efb7d31ce0a57e28f5f3946b` maps to
new `e1f226409b5ecf7c5e2639505e1df8e2f926c993`. Commit identities differ.
Preserve the new history; do not merge the legacy graph, mirror over current
branches, reset an occupied local branch, or force-push a synchronization.

The concurrent post-copy ports were independently checked by complete tree and
ancestry, preserving their matched rewritten bases with no raw legacy ancestors:

| Source revision | Destination revision | Content |
| --- | --- | --- |
| `43cbdc739493d34089261aabbc183550efee6b5f` | `a145c1fc53ba8fef2e9a66c5ccdebdc32514dcaa` | Updated adoption contract |
| `f5ee4a50d1e9308fe50c404081f75ea9603b8d61` | `c4e346974196357b60a9eb541b4f0e4c12f2954d` | Adoption-decision UI |
| `73b19801` | `db09c21064ce8f8bc20a3ca345bab3350827fe05` | Independent ACP mutation-test branch |

The independent final snapshot at `2026-10-24T05:07:54Z` (October 23 Pacific)
checked all **236 legacy branch names** against the new repository: every source
tree was present, and all **12 open legacy PRs** had matching destination PR trees.
ACP #3 `851ef3b2` and #5 `6e1c0c8` matched their latest legacy source trees; #3
was an ancestor of #5 and both preserved rewritten ancestry.

Other active PRs were reopened only after checking source/destination tree equality.
New changes can arrive after this checkpoint: fetch both repositories and inspect
current heads before syncing again. The new repository can legitimately contain
newer work; a tree difference is not permission to replace it with the old tree.

For a remaining legacy delta, record the exact source snapshot and an explicit
same-tree base pair. Apply the complete delta (including merge-only changes) to a
fresh branch/worktree derived from the destination base. If the destination also
advanced, reconcile both sets of changes and review the result. Verify the final
tree or an explicit reconciled diff, check destination advancement again, then
push without force. Record source/destination hashes and verification in #1.

## Preserved local work

[#7](https://github.com/HenryWashuHe/genesis-history-experiment/pull/7), commit `110a1073c680e4bbd67e2db1be0faff85fb789ee`, preserves
all twelve uncommitted repair source/test/doc files byte for byte on the matching
new forge base `f488eef`. It adds one provenance document. The old working
directory is unchanged. This is deliberately a draft against the forge branch,
not current main; historical WIP status edits do not supersede current docs.
Only the earlier happy path passed; the complete revised twelve-case target,
final checks and independent review remain open. Resume through a scoped
supporting milestone rather than displacing development methods.

Local-only old review compositions contain no additional implementation edits:
`380aba81` composed the core stack without running tests; `395477e4` composed ACP
and passed 135 bridge tests. Their source/regression branches are preserved.
Do not import those raw legacy merge histories or call those counts a new test run.

## Historical credential checkpoint — October 23, 2026

Production CREAO now uses [API Trigger](CREAO_RUNTIME.md), merged October 25.
The v1/v2 compatibility limitation below describes the migration checkpoint,
not current backend configuration.

The supplied `.env` is stored only in the ignored local checkout with owner-only
permissions. It is not part of this migration or any PR. Credentials are not
shared through issue comments, branch commits, transcripts or handoff files.

Read-only authenticated checks returned HTTP 200 for Apify `/v2/users/me` and
CREAO `/v2/agents`; CREAO returned zero hosted agents and no further page. The
sanitized local report is `data/live-gates/credential-readiness.json`. This proves
credential-backed metadata access, not Actor execution or agent core work.

The supplied `CREAO_V2_API_KEY` works with v2; the current Rust adapter requires
a v1 account key and pins `/v1`. Do not alias the names as a compatibility fix.
[Official v2 agent-list documentation](https://developer.creao.ai/docs/v2/agents/list)
distinguishes its authentication from v1. A v2 runtime/provisioning path needs
separate scoped implementation. ACP's reported Fetch discovery and failed Elastic
smoke are separate evidence under #1, not independently repeated here.
