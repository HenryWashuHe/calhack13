# Rehearsal: select, plan, receive, adopt and invoke (October 25, 2026)

The second local rehearsal. It replaces the first one's store-seam bootstrap
with the real intake → planner → kernel path. It was predeclared in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6393821881)
with spec sha256 `927dacc6…` before anything ran. It's author-run, uses one
host and loopback servers, and the planner is scripted. It closes no gate.

## What ran

Source: composition `fd24b83` on `henry-claude/rehearse-invocation`, which is
#186 `95bd764` (including the #179 fixes) merged with #65 `3e101e9`. Binaries
`lab-exec` `9a112687…` and `lab-score` `d384d1db…` (no executor or scorer
source changed since the first rehearsal). Sandbox: namespaces, as root,
pinned Python 3.14.0. Driver: `crates/genesis-planner/tests/rehearse_planner_chain.rs`
(ignored by default).

1. **Selection and responsibility** (`chain/search.json`, `intake.json`,
   `org-change.json`). A publishes its child `4f455dc6` to a loopback registry.
   B finds it by keyword search and posts it to
   `/missions/{m}/method-development` (#157). A `ReplayClient` planner (a
   scripted reply, not a model) proposes `CreateRole` "Method specialist" plus
   `AssignResponsibility` from a new requirement. The kernel commits it,
   triggered by the intake observation.
2. **Receiving trial** (`trial/`, run by the driver only after that commit).
   Child vs the fixed cart, rough-transport, seeds 42000..42005, margin one:
   **Accepted, 6/6 vs 3/6.** Model use measured zero.
3. **Adoption.** The receiver pins that trial's plan for the planner-created
   requirement. **Existing-owner control:** the lab lead, who isn't the
   requirement's owner, is refused twice, once naming the specialist's
   responsibility and once its own (409 `method_authority`;
   `lead-attempts.json`). The specialist adopts (`owner-decision.json`), with
   acceptance recomputed by the server.
4. **Invocation.** One run under the specialist's grant on seeds 42100..42105,
   through `invoke.py` and `lab-exec run` on the server-frozen plan: status
   **completed**, `output_generated`, budget `within_limits`, 41,107 steps,
   803 ms. For information, the submitted design `afdfba24…` succeeds on 6/6 of
   those instances under `lab-eval accept` (`chain/design-accept-42100.json`).

## What this is not

- Not model judgment. The planner's change is a scripted reply. A live
  planner call would be the next step, and it needs its own predeclaration.
- Not independent organizations: one operator and one host, with test
  registry tokens.
- Not G2 closure, a benefit claim or a live model run. #186, #179 and #65 are
  unmerged.
