# Real Sonnet 5.5 job-shop controlled rehearsal — 2026-10-25

## Evidence classification

This is **one completed, real-model, preregistered controlled rehearsal** on the
synthetic 10-job × 5-machine job-shop family. It used the reviewed runner at
[`b0373e38b01110bb2a691db970da17281b108e30`](https://github.com/HenryWashuHe/genesis-history-experiment/commit/b0373e38b01110bb2a691db970da17281b108e30)
(PR [#228](https://github.com/HenryWashuHe/genesis-history-experiment/pull/228), merged).
The [prospective registration](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6398339684)
preceded the first paid request; the protocol SHA-256 was
`161ac169cb70c02022c5210ac82634f6db9a4e42442ef7c825214afefc6c18e8`.
The [terminal journal head](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6398534809)
was published before independent replay. The
[issue #1 result note](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6398575427)
records the interpretation and limits.

The original 60-job × 6-operation Opus scheduling study was **not run**. This
rehearsal does not establish G4, cross-organization transfer, A→B→A recursion,
or a general capability-improvement result.

## Predeclared comparison and execution

Five paired seeds each had five generations in two arms: feedback iteration B
and independent-root generation A. Both used fixed Claude Sonnet 5.5 settings,
five model requests per seed and arm, paired selection panels, and one sealed
60-instance final panel per seed. A seeded random-rule search and fixed SPT,
MWKR, and LPT rules were controls. Success on one final instance meant
makespan ≤1.20 × the larger of machine load and job-work lower bounds. See the
[frozen protocol](../../PROTOCOL.md) at the pinned source for exact screening,
selection, budgets, and stopping rules. The study began at
`2026-10-25T13:44:25.943Z` and completed at `2026-10-25T13:56:15.692Z`.

| Seed | Feedback B | Independent A | B − A | Random search | Fixed MWKR |
| --- | ---: | ---: | ---: | ---: | ---: |
| 01 | 49/60 | 46/60 | +3/60 | 51/60 | 42/60 |
| 02 | 44/60 | 42/60 | +2/60 | 48/60 | 35/60 |
| 03 | 48/60 | 46/60 | +2/60 | 49/60 | 42/60 |
| 04 | 47/60 | 43/60 | +4/60 | 14/60 | 42/60 |
| 05 | 43/60 | 39/60 | +4/60 | 49/60 | 39/60 |
| Total | 231/300 | 216/300 | +15/300 | 211/300 | 200/300 |

The five paired differences were `[0.05, 0.0333333, 0.0333333, 0.0666667,
0.0666667]`. Their mean was **+0.05 (five percentage points)**; the predeclared
two-sided Student-t 95% interval was `[0.02930891764820195,
0.07069108235179805]`; all five differences were positive. Thus the narrow
predeclared B-versus-A rule passed. The interval relies on five seed-level
observations and the t approximation; it is not a cross-family estimate.
Feedback also had a lower final mean makespan ratio in all five pairs, a
secondary result. There were zero invalid final scores in either model arm.

**The random-search control is a material counterpoint.** It beat feedback on
four of five panels. Feedback's aggregate 231/300 vs random's 211/300 is
heavily driven by seed 04, where random search scored 14/60. We therefore do
not claim robust superiority to random search. The final-panel result also
does not demonstrate that every intermediate generation improved. Two rounds
retained three screened candidates rather than the nominal four (seed 03 B
round 2, seed 05 A round 2); no replacement calls occurred.

## Provider use, cost, and retained record

The journal records 50/50 real Sonnet 5.5 requests, 50 distinct provider
message IDs, 50 `end_turn` responses, 50 settled generation outcomes, 208
hash-linked English events, and no retry, fallback, or `run_stopped` event.
Provider-returned actual usage totaled 124,021 input tokens and 26,708 output
tokens, with no cache tokens. At the preregistered USD $2/$10 per million-token
rates, the **operator-calculated** charge was **USD $0.515122**. It is not a
Claude Console invoice. It stayed below the USD $20 run ceiling. The USD
$10.2048 full-run pre-request bound and USD $39.79 reserve for potentially
unposted prior project use are *accounting reserves*, not charges.

[`retained-evidence.tar.gz`](retained-evidence.tar.gz) contains the 114 retained
files: manifest, 50 request JSON files, 50 response JSON files, 10 arm states,
`events.ndjson`, `final-scores.json`, and `summary.json`. The archive is 337,003
bytes; its digest is in [`SHA256SUMS`](SHA256SUMS). A file-content scan before
publication found no `sk-ant-`, `ANTHROPIC_API_KEY`,
`GENESIS_LLM_API_KEY`, `CREAO_API_KEY`, `APIFY_API_KEY`, `Authorization`,
`Bearer `, `@gmail`, `api_key`, or `/Users/eric` string in the retained bytes.
The archive contains unmodified synthetic prompts, provider responses, and
model-generated rule source; treat it as research evidence, not executable
instructions.

## Independent local replay

Check the archive checksum, extract it into a private empty directory, and run
the verifier at **exact source commit `b0373e3`**. Do not invoke `run.mjs`;
verification is local and sends no provider request. From this report's directory
inside a Git checkout, for example:

```sh
cd experiments/jobshop-controlled/results/2026-10-25
shasum -a 256 -c SHA256SUMS
git worktree add --detach /tmp/genesis-jobshop-source b0373e3
mkdir -p /tmp/genesis-jobshop-evidence-replay
tar -xzf retained-evidence.tar.gz -C /tmp/genesis-jobshop-evidence-replay
node /tmp/genesis-jobshop-source/experiments/jobshop-controlled/verify.mjs \
  --output-dir /tmp/genesis-jobshop-evidence-replay \
  --expected-head 51105046823df6fd45fc8844197b36559102c1005f0940fad1571b054f242b01 \
  --recompute-final
```

The actual post-run independent replay exited 0 with
`status=complete`, `events=208`, `requests=50`, `settled=50`,
`spent_usd=0.515122`, and `recomputed_final=true`. It checks retained
request/response, journal and engine-ledger bindings, plus independently
recomputes every final score on the frozen panels. The separately published
terminal head anchors the local hash chain. This does not authenticate the
provider, prove the absence of other account use, or constitute a second-host
replication. The user explicitly waived waiting for additional Linux
validation/local generated-rule-risk assurance for this rehearsal.
