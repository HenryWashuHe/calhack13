# Rehearsal 6: live planner, second attempt (October 25, 2026) — predeclared

**Nothing has run.** This is rehearsal 4 again with its two infrastructure
causes removed, so a live model actually makes B's ownership decision.

## What changes from rehearsal 4

- The driver passes `HTTPS_PROXY`, `https_proxy`, `NO_PROXY`, `no_proxy` and
  `SSL_CERT_FILE` to the planner, beside `GENESIS_LLM_*`. No HTTP proxy
  variable: B is plain HTTP on loopback.
- The branch composes #242 (reqwest trusts the system store). #242 is not
  merged; its ignored network test reached the provider's 401 with a fake key
  from this host.
- The trial ID and request labels come from `spec.json`, so this run's seeds
  and names don't reuse rehearsal 4's.

Everything else is rehearsal 4's flow (issue #1 6394413035): registry, A and
B as separate processes; A publishes the child `4f455dc6`; B's reader token
publishing as A is refused; B searches and posts the intake; the real
`genesis-planner once` plans the intake observation with project model
settings (`anthropic`, `claude-opus-5-5`), one `once` call, whatever it
decides is retained.

- If it commits a requirement: B runs `rehearse6-accept-vs-fixed-cart`
  (child vs fixed cart, rough-transport, 6 instances from **49000**, margin
  one), restarts with the receiver pin, the requirement's owner decides (A's
  token and, where someone else owns the work, the lead are refused), and one
  invocation runs on rough-transport **49100..49105** and is scored.
- If it makes no change or escalates: the run stops there and that is the
  result.

## Reading

The point is the model's ownership decision on an ordinary observation:
which role (new specialist, existing owner, or no change) and why, as the
planner records it. One call, one observation, one model. It's a rehearsal
of the path, not a measurement of planner quality, not G2 closure, not a
benefit claim. A failure to reach the model again is reported as such.

## Pinned inputs

- Branch `henry-claude/rehearse-live-planner-2` (rehearsal 4's branch + main
  `64de3e3` + #242).
- Driver `crates/genesis-planner/tests/rehearse_live_planner.rs` sha256
  `ab02b9478246582e3f1a47049b46a7f76027bfed9a82e9e8a9b3df2381108fb1`.
- `spec.json` `e24a3ae9…`, `invocation-template.json` `b2a0a8b6…` (made with
  `lab-exec example-plan namespaces 6 72000 60000 … rough-transport
  <child> 49100`, current lab binaries `72836d98…` / `5e969c17…`).
- Pinned Python 3.14.0 for the sandbox; root.
- Cost bound: one planner call (its client may retry twice on transient
  errors); no other model use.
