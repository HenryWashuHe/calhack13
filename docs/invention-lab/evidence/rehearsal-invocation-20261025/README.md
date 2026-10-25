# Rehearsal: receive, adopt and invoke under the grant (October 25, 2026)

A local rehearsal of the method-adoption → authorized-invocation → execution
chain, with real processes at every step. It was predeclared in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6393313105)
with spec sha256 `065cfdb7…` before anything ran. It's author-run, uses one
host and loopback servers, and closes no gate.

## What ran

Source: composition `e2d72468` on `henry-claude/rehearse-invocation`, which is
#186 (method runs, stacked on #185 and #179) merged with #65 `3e101e9`. The
binaries are `lab-exec` `9a112687…` and `lab-score` `d384d1db…`, built from it
(the later commits add only this directory and the rehearsal test).
Sandbox: namespaces, as root, runtime ref `0b21e795…` (pinned Python 3.14.0).
Host: Linux 6.18.44.

1. **Receiving trial** (`trial/`, from `receive.py spec.json trial`). B runs
   A's revised child `4f455dc6…` (model-generated, never edited) against the
   fixed cart `dc5b74f3…` on rough-transport, fresh public seeds
   41000..41005. 72,000 steps / 60 s per slot, criterion margin one. Result:
   **Accepted, 6/6 versus 4/6.** Model calls are measured zero in both slots.
2. **Adoption** (`invocation/adoption.json`). A publishes the child to a
   loopback registry. B's owner role adopts it through `POST /method-adoptions`,
   which recomputes acceptance from B's retained trial under B's pinned plan.
   `authorization.json` is the current grant.
3. **Invocation** (`invocation/run/`, via `invoke.py prepare` and `run`). The
   operator proposed a one-slot invocation plan naming the grant on fresh
   seeds 41100..41105 (`proposed-plan.json`, instances from
   `invocation-template.json`). The server froze it, started it once, and
   `lab-exec run` ran the frozen plan in the namespace sandbox. The receipt
   was accepted: status **completed**, outcome `output_generated`, budget
   `within_limits`, 26,967 simulation steps, 718 ms, model use measured zero.
   `run-record.json` is the server's record.
4. For information only: the submitted design `1cc0dd6e…`, scored with
   `lab-eval accept` on the same six instances, succeeds on 6/6
   (`invocation/design-accept-41100.json`). Attaching a scored output to the
   responsibility as work done isn't implemented, so this isn't that.

`crates/genesis-server/tests/rehearse_invocation.rs` (ignored by default)
performs steps 2 and 3. It writes `invocation/` and refuses to run if
`invocation/` exists.

## What this is not

- Not independent organizations. One operator ran both, on one host. A's
  publication used test registry tokens.
- B's mission, roles and requirement were bootstrapped through the store seam,
  not the planner, so no organizational judgment is shown.
- Not a live model run, not a benefit claim, and not G2 closure. #185/#186
  are drafts whose shared contract Codex hasn't agreed. #65 and #179 are
  unmerged.
- Rough-transport is B's task family. A's family isn't involved.
