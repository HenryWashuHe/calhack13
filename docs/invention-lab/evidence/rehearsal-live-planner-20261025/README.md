# Rehearsal 4: live planner (October 25, 2026) — did not reach the model

Predeclared in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6394413035)
at `8cc3594` before it ran. It ran once, as predeclared, and **failed before
any model request reached the provider.** No model judgment was observed.
Author-run, one host, loopback.

## What happened

Steps 1–3 of rehearsal 3 ran as before in separate processes: A published its
child, B's reader token publishing as A was refused (`orgs/b-publish-as-a.json`),
B searched and posted the intake (`orgs/intake.json`). The prompt for the
intake observation was rendered without a model call (`orgs/planner-prompt.txt`,
7,509 bytes).

Then the real `genesis-planner once --observation <id>` ran with project model
settings (`anthropic`, `claude-opus-5-5`). It made one attempt; the client
retried twice on a transient error and gave up:

```
Escalated { reason: "model call failed: model request failed: connect", attempts: 1 }
```

`orgs/planner-once.stdout` holds the full output. B's organization afterwards
has no requirement (`orgs/b-organization-after-planner.json`), so the driver
stopped there as predeclared. No receiving trial, adoption or invocation ran;
seeds 46000.. and 46100.. were not used.

## Why

My driver cleared the planner's environment and passed only `PATH` and
`GENESIS_LLM_*`. This session reaches the internet only through a local
HTTPS proxy, so the planner tried to connect directly and couldn't. That's a
harness error on my side.

Passing the proxy through would not have been enough either. The workspace
builds `reqwest` with `rustls-tls` and its bundled webpki roots, so the planner
trusts only those roots, not the system trust store where this session's proxy
CA is installed. From this container the planner can't complete TLS through
the proxy without a change to its TLS root configuration, which is planner
code, not mine.

Cost: zero model requests reached the provider, so there's no token usage.
The planner doesn't record usage in any case.

## What this is not

Not a model decision of any kind, not a planner defect, not evidence about
adoption. The next attempt needs either the planner built to trust the system
store, or a run from a machine with direct egress. Either needs its own
predeclaration.

`SHA256SUMS` covers the committed files. Process logs and the SQLite files are
not in Git.
