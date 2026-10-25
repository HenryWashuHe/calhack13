# Independent Linux validation, round 2 — progress

Target: origin/main `85073cc6208800e15b5cc96b742bfba07ef325b9` (#65 and #57 merged).
Host: root, Linux 6.18.44, same host as round 1, separate agent. No network or credentials.
Interpreter: `/usr/lib/py314/bin/python3.14` (3.14.0), symlinked as `/usr/local/bin/python3` only during runs.

On main, `executor/src/run.rs` and `scoring/src/hidden.rs` are identical to `f9c3dc6`.
`executor/src/sandbox.rs` matches neither `d57efc8` nor `f9c3dc6`: it combines #65's
minimal-root chroot, `close_range`-required descriptor policy and #57's termination evidence.

## Done
- [x] Fetched main, recorded sha, created worktrees.

## Left
- [x] Executor suite as root (`executor-85073cc.log`): test/clippy/fmt exit 0. 48 tests: 8 lib (7 `termination_evidence_tests`) + 40 integration, 0 failed/ignored. Root-gated suites ran: namespace_lifecycle 3, namespace_stop_without_kill 1, review_namespace_escape 7, sandbox 1. No `nobody` processes after. `lab-exec` sha256 in `BINARIES.sha256`.
- [ ] Scoring suite as root, plus strict clippy and fmt (raw log here).
- [ ] `indep_hidden_closure.rs` with the positive case asserting a `Scored` outcome (file added here; run pending).
- [ ] sha256 of the tested `lab-exec` / `lab-score` binaries.

## Not done in this round (scope note)
I am not reworking the round-1 escape-probe harness (`indep_linux_probe_orch.py`), as the
coordinator's items 1–4 asked. Its namespaces-mode "blocked" results have no in-sandbox handshake.
The daemon-heartbeat, env and errno channels pointed at paths or fds the sandbox removes, so
they were not observable. Treat those namespaces-mode results as **inconclusive**. Only the
none-mode controls are positive observations.

Correction to round 1: the author suite does cover the deadline path
(`namespace_lifecycle.rs::a_detached_descendant_does_not_outlive_a_namespace_run_killed_at_its_deadline`).
My round-1 claim that it didn't was wrong.
