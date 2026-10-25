# Rehearsal #230 closeout review

Reviewed PR #230 at `d9db954adad1ecf2b125863a1c5c8dca72df3a30` using
source, committed JSON and byte hashes only. This review did not run the
rehearsal, a method, evaluator, scorer, sandbox, provider, or OS probe.

## Driver correction

The receiver restart evaluated `a.spawn(...)` inside `std::mem::replace`
before dropping the old `Proc`. Both children used A's same port, so the
replacement could fail to bind before `Proc::drop` killed and reaped the old
listener. The correction matches #215's `c25fb087`: explicitly drop the old
receiver, then spawn and retain its replacement, then wait for readiness.

The historical driver is unchanged at its predeclared commit `0b8eba5`:
SHA-256 `f5feec4bfb0b98595448c4e161488d062aef2a092e11efd49d93c3321adf4dd8`.
The corrected driver has SHA-256
`aaa67264e13b4f9d8068207700f8babc31766a2f89b9791498adf03f1904d303`.
No new rehearsal result is claimed for this correction.

## Retained evidence

All 179 entries in the historical `SHA256SUMS` match their committed bytes.
They cover all 180 files in
`docs/invention-lab/evidence/rehearsal-aba2-receiving-20261025/` except the
manifest itself. The manifest's SHA-256 is
`1b3c70df430e9287d7c48735b9e153d96ae58cedc58ff9a88a1699aecc0c716c`.
All 180 historical files remain byte-identical to the reviewed head.

The retained per-instance metrics support B's 11/12 versus 10/12, A's 3/12
versus 2/12, and the invocation's 2/12. The invocation receipt records 22,500
simulation steps. The eight acceptance-file digests and exact method,
receiver-plan, adoption, requirement and grant bindings agree with the saved
records. The reported random-control comparisons (B: 11 versus 4; A: 3
versus 6) agree across retained summaries and verdicts; their raw scorer
artifacts are not included, as the historical README describes.

The historical README correctly identifies a stand-in method, scripted
`ReplayClient`, one host, loopback processes and dev-stub providers. Runtime
confinement, binary execution and timing remain author-reported. These
records neither complete a goal gate nor demonstrate recursive benefit.
The historical registry finding is preserved rather than rewritten to
reflect later publisher-precondition work.

## Validation

Compilation only, passed:

```sh
CARGO_TARGET_DIR=/tmp/genesis-method-result-review-target CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 /Users/apple/.cargo/bin/cargo check --locked -p genesis-planner --test rehearse_aba2
```

The following source-only ordering assertion rejects the original driver and
passes the corrected source; it launches no child process:

```python
import re

def check_restart(text):
    section = text.split('let receiver_path = state.join("a-receiver.json");', 1)[1]
    stop = re.search(r'\bdrop\(a_proc\);', section)
    start = re.search(r'let _a_proc = a\.spawn\(&bin, &registry_url, Some\(&receiver_path\)\);', section)
    ready = re.search(r'wait_up\(&a\.url\(\)\)\.await;', section)
    assert stop and start and ready
    assert stop.start() < start.start() < ready.start()
```

`git diff --check` passes. The ignored rehearsal test remains ignored; no
evidence was regenerated. The historical result is suitable for retention as
an author-run rehearsal with these limits, not as independently executed proof.
