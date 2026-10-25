# Judged-demo job-shop controlled rehearsal (prospective)

Status: **design only; no calls or outcomes under this protocol yet.** This is a
controlled rehearsal of the 10-job, 5-machine rule-generation mechanism displayed
in the Stage Lab beat. It is separate from the prospective 60-job scheduling study
and cannot establish cross-organization transfer or G4.

## Question and fixed comparison

Does giving a fixed-weight Claude Sonnet 5.5 mutator attributed, same-family
feedback across five generations produce better dispatch rules on fresh job-shop
instances than spending the same model-call and local-evaluation budget on
independent generations? A success is makespan no more than 1.20 times the job or
machine load lower bound. Lower makespan ratio is better.

Five paired repetitions use seeds `judge-20261025-01` through
`judge-20261025-05`. Each repetition has two arms, both using the exact ACP
`jobshop-10x5` family, engine, cascade and sandbox source on the pinned PR #216
head. Each arm gets five generations, one model request per generation, eight
proposed rules screened on three separate instances, the top four submitted to
the engine, and a 20-instance fresh sealed selection panel per generation.
Selection compares candidates with the current incumbent using the engine's
one-success margin. Both arms keep the best incumbent. The only difference is
what the mutator sees:

- **Independent sampling (A):** each request starts from the frozen SPT root and
  sees no prior scores, candidate archive or reflection. External selection may
  keep a winner, but the generator receives no history.
- **Feedback iteration (B):** each request sees the current parent, scored
  programs, id-free per-instance feedback and reflection supplied by the ACP
  mutator. The instruction supplies no strategy direction.

The same repetition/round panel seeds and evaluator settings are used by both
arms. Arm order alternates B/A, A/B, B/A, A/B, B/A across repetitions. A
response that arrives (including a refusal, an empty or unparseable answer, or
a `max_tokens` stop) is counted and retained, never retried or replaced, and
the run continues. A request whose provider outcome isn't a successful response
(an HTTP error status, a timeout, a lost connection) **stops the run**, with
its full reservation charged and its HTTP status and error type, when known,
recorded in the stop event; it is never retried. No outcome-driven extension,
seed replacement or other stopping occurs.
The `stall` shortcut is disabled. A complete run is 50 model requests.
Each repetition has a public prompt label `repetition 1` through `repetition 5`
shared by its A/B arms. It differentiates independent model samples but is
unrelated to the hidden instance seeds, which are never placed in a prompt.

Within each repetition, the existing fixed SPT, MWKR and LPT controls and one
seeded random linear-rule search are frozen before model requests. The random
search receives 620 local scoring evaluations (124 rules x five private training
instances), matching each arm's nominal 500 selection plus 120 screening
evaluations. The control does not see the selection or final panels. Controls
are report-only during the generation loop and are scored on the final panel.

After all requests and selections are sealed and the run ledger checks pass,
open one 60-instance final panel per repetition, paired across arms and all
controls. Final values cannot change an incumbent, prompt or sample size.
Invalid or failed final scores count as unsuccessful; mean makespan ratio is
reported as unknown for that arm if any ratio is unavailable, with invalid and
failure counts retained. The primary endpoint is the paired difference B minus A in final success
fraction per repetition. Secondary endpoints are mean makespan ratio, the best
fixed rule, random search, generation-by-generation selection curves, failures,
local evaluations, model tokens, charge and wall time. Report the five paired
differences, their mean and a two-sided 95% Student-t interval (`df=4`,
`t=2.776`), with its small-sample assumptions. A positive demonstration
requires a mean success-fraction gain of at least 0.05, a lower interval bound
above zero and at least four positive repetitions. Any other result is null,
negative or inconclusive as appropriate. It cannot alone prove a reusable
development-method improvement or the GENESIS A-to-B-to-A return.

## Model, charge and failure gate

Model ID `claude-sonnet-5-5`, `output_config.effort=medium`, maximum 8,000
output tokens, non-streaming Messages API, no cache/batch/server-side fallback,
same prompt template and limits in both arms. The model returned by every
response must match the requested ID. The generator's variable content differs
only as prescribed by the arm. Request and response hashes, actual provider
usage, status and identity are recorded in English.

[Anthropic's current listed price](https://platform.claude.com/docs/en/models/sonnet-5-5/overview)
is USD $2/MTok input and $10/MTok output. Limit the complete request JSON to
30,000 UTF-8 bytes. Reserve 62,048 input tokens (twice that byte limit plus
2,048 overhead) and 8,000 output tokens per request: USD $0.204096 per call,
USD $10.2048 for all 50 calls, inside this run's USD $20 ceiling. The full
USD $10.2048 reserve is required before the first request. After each response,
settle only from provider actual usage; an unknown submission or usage stops the
run with the full unresolved reservation charged against admission. Stop if
actual charge or remaining reservation could exceed USD $20 or the earlier
USD $50 total project-study ceiling. No retries, unapproved cloud hosts or
unrecorded paid requests. This is an operator-side dollar bound from stated
prices, not an Anthropic account spending limit.

The Claude Console organization cost page displayed USD $0.00 for the last
30 days as of 2026-10-25 06:54 UTC, but the page explicitly says recent usage
may take hours to appear. Before launch, reread it and reserve at least USD
$20 for unposted prior project usage, subject to any higher observed amount.
This is a conservative accounting reserve, not an assertion that USD $20 was
charged. Concurrent use of the same project key must be excluded during the run.

The run retains an append-only, hash-linked request/outcome journal; the ACP
engine ledger is saved and verified after each generation. The final panel
stays sealed until all 50 request outcomes are known and the selected rule
hashes are frozen. A crash, missing record, unknown provider charge, model
mismatch or ledger failure stops the run without opening the final panel.
Before reading a key or constructing controls, this entrypoint rejects a dirty
checkout and any host without the recorded macOS `sandbox-exec`/network-denied
mode, and refuses a key file that lies under any path the rule sandbox may
read (its runtime roots, including the prefix the node binary is installed
under, and the iteration scripts). The manifest pins the reviewed commit, all loaded source file bytes,
Node executable digest/version, OS release/architecture and observed sandbox
description. A trusted root rule must also score one preflight instance inside
that sandbox before the key is read. The replay verifier checks those bindings.
This host gate does not
claim that the still-reviewed sandbox policy is stronger than its actual rules.
All process and result records are English. Existing ACP smoke results,
offline tests and this run are reported separately.

After completion, publish the terminal event head separately, then run
`node experiments/jobshop-controlled/verify.mjs --output-dir DATA_DIRECTORY
--expected-head PUBLISHED_HEAD --recompute-final` against the exact source
commit. This rechecks request/response and cost binding, engine ledger heads,
selected packages and every final score. A matching local hash chain detects
altered retained bytes only when bound to the externally published head; it
does not authenticate the provider or certify that no unobserved request was
sent from another process.

The user waived waiting for additional independent Linux validation and
accepted the local generated-rule execution risk for this run. The exact
runtime sandbox and source head must still be recorded; this waiver does not
turn earlier unverified or author-reported results into independent evidence.
