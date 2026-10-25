# Rehearsal #243 closeout review

Reviewed PR #243 at `efe7b93ba10b239da0987ddb5dc001d0d017f514` using
source, committed JSON and byte hashes only. This review did not run the
driver, a method, evaluator, scorer, namespace probe or provider request.

## Driver correction

The receiver restart evaluated `b.spawn(...)` inside `std::mem::replace`
before dropping the old `Proc`. Both children used B's same port, so the
replacement could fail to bind while the old listener remained alive.
The correction explicitly drops the old receiver, whose destructor kills
and waits for it, then starts and retains the replacement before waiting for
readiness. It makes the same ordering correction as #236.

The restart occurs after successful planning and a receiving trial; the
retained failed run never reached it. This correction changes no historical
result and supplies no new successful rehearsal evidence.

The original predeclared driver at `8cc3594` and the reviewed PR head have
SHA-256 `d4df048769e43bbc482b3aa4f379692253cbbb32d1550f386aee1d443d20c0f9`.
The corrected driver has SHA-256
`b206925c69dc98ad0a631cfb171d04e6a07e1cc83382ae9a5958d5f3f11d9e4f`.

## Retained evidence and limits

All 15 entries in the historical `SHA256SUMS` match their committed bytes;
they cover all 16 files in
[the archive](../invention-lab/evidence/rehearsal-live-planner-20261025/README.md)
except the manifest itself. The manifest's SHA-256 is
`653023efb75cabc383d1cfb87858b2a03fa432be044512d9089f8a0ce33275e3`.
All 16 historical files remain byte-identical. The predeclared spec and
invocation template also remain unchanged from `8cc3594`.

Both referenced packages match their declared hashes. The searched candidate
equals the accepted intake selection; the canonical intake hash, B's
observation, organization and mission, plus the selected runtime, protocol
and dependencies, agree. The retained prompt is 7,509 bytes. B's saved
organization has no requirements, and no subsequent trial, adoption or
invocation output is retained. The planner log records two connection-error
retries followed by escalation. The author's zero-provider-request account
is not an independently authenticated provider or billing measurement; no
model response or token-usage measurement is retained.

The historical README's last sentence says process logs are not in Git.
That is inaccurate: `org-a.log`, `org-b.log` and `registry.log` are committed
and manifested. SQLite state files are absent. The hashed README is preserved
as historical text; this note records the correction. The linked
[predeclaration](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6394413035)
precedes the separate
[author result report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6394711367).
These records establish no model judgment, adoption or completed goal gate.

## Validation

Compilation only, passed:

```sh
CARGO_TARGET_DIR=/tmp/genesis-method-result-review-target CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 /Users/apple/.cargo/bin/cargo check --locked -p genesis-planner --test rehearse_live_planner
```

The source-only ordering assertion below rejects the original driver and
passes the corrected source. It launches no child process:

```python
import re

def check_restart(text):
    section = text.split('let receiver_path = state.join("b-receiver.json");', 1)[1]
    stop = re.search(r'\bdrop\(b_proc\);', section)
    start = re.search(r'let _b_proc = b\.spawn\(&bin, &registry_url, Some\(&receiver_path\)\);', section)
    ready = re.search(r'wait_up\(&b\.url\(\)\)\.await;', section)
    assert stop and start and ready
    assert stop.start() < start.start() < ready.start()
```

Scoped `rustfmt --check` and `git diff --check` also pass. The ignored rehearsal
remains ignored. Evidence was not regenerated.
