# Scheduling study: protocol and current evidence

**October 25 status:** this 60-job/6-operation Opus study was not run. The
separate [10-job/5-machine Sonnet rehearsal](../../experiments/jobshop-controlled/results/2026-10-25/README.md)
is completed and published. The readiness review below is historical: #144,
#57/#65, #180/#181 and the verifier corrections are now merged; independent
runtime assurance and G0–G4 remain open. Preserve this study's proposed protocol
and original calibration records; the smaller rehearsal does not satisfy it.

**Status:** A prospective experiment with one completed **offline calibration**. No model-generated scheduling method, outer iteration, final hidden-panel run, or cross-organization transfer has been executed.

## Scientific question

With fixed model weights and matched total resources, does repeated feedback-driven improvement of a development method produce better reusable scheduling policies on fresh flexible job-shop instances than equal-budget independent sampling? Does retaining two parents improve on a single-parent feedback loop?

The first study is within one synthetic task family. Its three strata are not separate transfer families. A positive result would not by itself establish a GENESIS A → B → A method return or a network-wide recursive benefit.

## Task and immutable study boundary

The local fixture generator `experiments/scheduling-study/benchmark.py` defines `scheduling-study/1`: 60 jobs, six ordered operations per job, 12 machines, two to four eligible machines per operation, integer durations from 1 to 30, and three declared strata (`balanced`, `bottleneck`, `restricted`). A deterministic serial decoder converts a bounded six-feature linear policy into exactly 360 scheduled operations. The separate `independent_checker.py` rejects missing or repeated operations, wrong machine or duration, job-precedence violations, machine overlaps, malformed numbers and time-limit violations. A feasible schedule is scored by makespan.

The policy interface and trusted decoder are intentionally much narrower than arbitrary Python. A future generated **method** would search this fixed policy space using a declared development budget, then submit one reusable policy. The model may change its method's search logic, but not the generator, policy grammar, decoder or checker. A generated method must run behind the project’s reviewed executor and durable experiment ledger before its result can count as formal method evidence. The current local pilot executes only trusted repository code and static JSON policies.

The generator currently uses SHA-256 counter draws, so a seed and stratum reproduce an instance without depending on Python's random sampling behavior. Exact source hashes are stored in each local run manifest. The study deliberately excludes rush orders, deadlines, failures, preemption and setup times. Its result is about this declared synthetic distribution, not deployment to an actual factory.

## Planned formal comparison, pending readiness freeze

| Arm | Model proposals | Feedback retained |
| --- | --- | --- |
| A: independent sampling | Four proposals per round, each starting from the common initial method | No prior method feedback enters a new proposal; the external selector retains the best evaluated method |
| B: single-parent iteration | Four children per round | One incumbent and its same-family attributed feedback |
| C: two-parent iteration | Four total children per round | Two retained parents and their same-family attributed feedback |

Frozen handwritten rules and a budget-matched random policy search are additional controls. Every arm uses the same exact model version and settings, maximum proposal count, context/output limits, inner development evaluation ceiling, local CPU/wall/memory bounds, and complete failure accounting. The default prospective design is 10 rounds × four proposals × three model arms × five independent repetitions: at most 600 model-generation requests. It is **not** a recorded run or a finalized financial ceiling.

For matched local computation, all three model arms should retain two externally selected archive slots, even though A's generator sees no prior feedback and B's generator sees only the first parent. C's generator can see both. Each round therefore scores the same seven slots per arm: four new proposals, two archived methods and one frozen control. Any repeated archive/control slot is a fresh method run and must be charged; it is not free because its source is old. The common initial two slots must be byte-identical across arms. This seven-slot schedule and the resulting complete local-call allocation require review before the formal freeze; a smaller cost accounting that omits repeated parents would invalidate the equal-budget comparison.

Each method run is planned to receive 200 policy-instance development evaluations over a public pool of 20 instances. A round compares parents, children and controls on the same 32 fresh selection instances. A separate post-freeze audit panel compares saved checkpoints; its values cannot select a winner. A 200-instance final panel per repetition is opened once after winners and decision rules are frozen. All new panels are paired across arms within a repetition. Missing, invalid, timed-out and unknown outcomes remain in the ledger rather than being silently excluded.

The seven-slot schedule would entail 1,050 charged method runs and up to 210,000 inner development evaluations across five repetitions before final reruns and audit checkpoints. It would also entail 33,600 round-selection evaluations. These are planning counts, not observed execution; output artifacts and every retry/failure still need exact admission and metering. The agreed implementation must reserve the complete total before the first formal method call.

The proposed primary comparisons are C versus A and C versus the strongest development-frozen handwritten baseline. B versus A and C versus B are declared mechanism comparisons. The practical success target under review was a mean makespan reduction of at least 5%, complete final feasibility, at least four positive independent repetitions out of five, and no predeclared stratum losing more than 5%. The uncertainty method and sample size must be frozen after local baseline variance and cost calibration, before model outcomes are observed. The unit of model-iteration replication is a full evolution repetition, not an individual held-out job set.

The review-approved starting analysis plan uses five full repetitions and no result-driven extension. For each paired final instance, define improvement over a given control as `(control_makespan - candidate_makespan) / control_makespan`. Average those improvements within each repetition, then average the five repetition means with equal weight. For the two primary comparisons, report Bonferroni-adjusted two-sided 97.5% Student-t intervals across the five repetition means (`df = 4`) and their assumptions. Passing requires a point estimate of at least 5%, an adjusted lower bound above zero, at least four positive repetitions, complete feasibility, and no predeclared stratum mean below -5%. A lower bound above zero does not establish a guaranteed 5% gain. With only five repetitions, wide intervals or inconclusive results are expected possibilities and must be reported as such. B-related comparisons remain secondary. The final statistical wording and strongest baseline identity still need owner review before the formal freeze.

An author-run schedule or attractive training curve cannot replace final confirmation. If only the random-search baseline is beaten, the result does not isolate iterative accumulation. If selection scores improve but final scores do not, report a null or negative generalization result. Do not add rounds or seeds in response to a near miss.

## Offline calibration completed

The first local calibration used seed `81000`, 20 development instances and 32 separate calibration instances. It evaluated four fixed policies and 40 random policies on all development instances (880 evaluations), selected the random candidate using **development results only**, then evaluated four fixed policies plus that selected candidate on calibration instances (160 evaluations). Every schedule was independently checked. This pilot's random-search budget exceeds the planned 200-evaluation **per-method** formal ceiling; these numbers only calibrate task difficulty, runtime and logging behavior. They cannot be used as a formal, matched-budget comparison.

| Policy | Calibration mean makespan | Feasible instances |
| --- | ---: | ---: |
| Earliest finish | 485.78125 | 32/32 |
| Shortest duration | 1275.90625 | 32/32 |
| Remaining work first | 1761.625 | 32/32 |
| Least loaded machine | 1396.96875 | 32/32 |
| Development-selected random candidate `random_005` | 408.28125 | 32/32 |

This is one synthetic calibration run. The selected random rule is about 16% below the earliest-finish mean on these 32 instances. The single run does not estimate formal uncertainty or show that model iteration helps.

Five additional **offline calibration** repetitions used the planned 200 development evaluations for a ten-candidate random search (ten policies × 20 instances), plus the same four fixed policies. Each repetition then used its own 32-instance calibration set. Their event files were independently replayed and verified; each has 440 evaluations, 472 events, zero model calls and zero provider calls.

| Seed | Earliest-finish mean | Development-selected random mean | Random result versus earliest finish |
| ---: | ---: | ---: | --- |
| 83000 | 480.3125 | 1320.65625 | Worse |
| 84000 | 473.625 | 412.28125 | Better |
| 85000 | 482.90625 | 1009.0625 | Worse |
| 86000 | 484.59375 | 403.78125 | Better |
| 87000 | 486.46875 | 416.96875 | Better |

The wide variation is largely policy-search outcome variation, not simply variation in the fixed earliest-finish score. Consequently, five independent evolution repetitions may give a wide interval for a 5% effect. This pilot cannot establish the variance of future model-guided iteration. Freeze a stronger fixed control and the repetition count before formal results exist; do not adapt the sample size in response to formal outcomes. A prospective fixed control candidate from these calibration runs is seed 86000's `random_009`, with weights `finish=1, duration=3, remaining=-2, machine_load=3, flexibility=0, wait=-1`. Its provenance is local calibration; it is not a model-developed method or a formal win.

Local retained files, excluded from Git by the repository's `data/` ignore rule:

```text
data/scheduling-study/pilot-seed81000-40/manifest.json
data/scheduling-study/pilot-seed81000-40/instances.json
data/scheduling-study/pilot-seed81000-40/events.jsonl
data/scheduling-study/pilot-seed81000-40/summary.json
```

The manifest binds source hashes, instance counts, selection rule, and zero model/provider requests. The instance bundle retains every synthetic input. The append-only event file records run start, frozen instances, each candidate policy, every per-instance score and schedule hash, candidate selection, and run completion. Each event carries a sequence number, previous-event hash and its own hash; writes are flushed and synchronized. It is an operator-local integrity and completeness check, **not** an authenticated external chronology or a substitute for the governed experiment ledger.

The five additional run directories are `data/scheduling-study/pilot-seed83000-10/` through `data/scheduling-study/pilot-seed87000-10/` in steps of 1000; each retains the same four file types. Their verification event-head hashes, in seed order, are `659cffa5ab551f2e206e920100e2ed21a5c513c29c47add57f87e1b0d846e07b`, `4e8db2281f7d20e67a83c1f6c0c2f980931104ada068e117f902bc07e307c431`, `2fdd53c6df5bb3fc5349bbca2eb9606b904ae73d4fc452e66324b275cd658bc4`, `afba71d4970522fd3b2222747107359ba674d37fc1a88c4ba75e0c1d9ac3201c`, and `b143809c05503b3715e0387ea844fdb45fc78dba308e118c56332ba4d220bc6e`. The seed 85000 digest corrects a truncated transcription in the first published protocol; the retained run bytes did not change.

`verify_run.py` independently replays all 1,040 evaluations, rechecks schedules and aggregate counts, validates source/instance/policy/summary hashes and the complete chain. Verification observed 1,132 events and finished at event hash `425fc486aafad1d9eb129eb6fc3cd2b4b5d583f58a51283bb6f9ae84b533900a`; summary hash `3ca86fc1b8438e1589fb3e87b75caddd7ff55d1da83336f4532dcdaa0c1ed70c`. The checker also has tests that mutate schedules and the journal to confirm rejection behavior.

After independent review of PR #201, the verifier also regenerates every declared instance from the manifest seed, requires the exact three source-hash entries, reconstructs every fixed and seeded-random policy, and enforces the complete event lifecycle. Selection must follow all development evaluations and precede every calibration evaluation; a failed run cannot also verify as completed. The reviewer's five original guard tests are retained unchanged alongside the six original tests. All 11 tests pass, and the stricter verifier accepts all six pre-existing local calibration runs with their original event-head hashes.

## Reproduce the local calibration

From the repository root on Python 3.14:

```sh
python3 -m unittest discover -s experiments/scheduling-study -p 'test_*.py' -v
python3 experiments/scheduling-study/run_local_pilot.py \
  --output-dir data/scheduling-study/pilot-seed81000-40 \
  --seed 81000 --random-candidates 40
python3 experiments/scheduling-study/verify_run.py \
  --output-dir data/scheduling-study/pilot-seed81000-40
```

For the **original retained run only**, independently compare against its previously published event head:

```sh
python3 experiments/scheduling-study/verify_run.py \
  --output-dir data/scheduling-study/pilot-seed81000-40 \
  --expected-head 425fc486aafad1d9eb129eb6fc3cd2b4b5d583f58a51283bb6f9ae84b533900a
```

The output directory must not already exist. To reproduce on a machine that already has it, choose a new directory name while keeping the same seed and code. Timestamps and elapsed times will differ, so event and summary hashes will differ; instance hashes and scores should match.

## Logging and monitoring assessment

The merged core provides exact trial plans, experiment-call reserve/settle/seal records, generation checkpoints, rosters, optional admission records and offline cost joins. Those components **do not automatically record every step of this proposed 600-request study**. The README and ongoing goal still list exhaustive history, runtime entry consumption, provider usage binding, whole-chain enforcement and Linux namespace assurance as open. The proposed ACP iteration engine is unmerged and has a current review hold; Henry owns executor/scorer fixes. The new local journal covers only the offline calibration shown here.

Formal operation therefore requires both instrumented writes **and active monitoring**:

1. Before each model request, persist its exact role, round, parent identities, prompt hash, model/settings, output ceiling and reservation. Persist a terminal success, failure or unknown observation without free retries.
2. Before each method execution or policy-instance evaluation, persist the run identity, exact input hash, grant and pending state. Settle it with independently measured steps, wall time, result hash and failure status.
3. After every round, compare expected proposals and every repeated parent/control run with observed records. Stop on a gap, unknown usage, mismatch, budget overrun, executor uncertainty or missing source.
4. Before opening the final panel, verify all method/package hashes, selections, parentage, panel seed commitments, aggregate costs and termination decisions. Freeze the winner and produce a read-only audit.
5. After final scoring, rerun a verifier over all retained events and artifacts. Publish English result tables with uncertainty, failures and costs, including null and negative outcomes.

Current automatic recording alone cannot provide that assurance. The operator must monitor the chain and stop when the record is incomplete. The local pilot demonstrates a small English-only event journal and verifier, but its journal must not be mistaken for the full governed infra record.

## Remaining readiness gates before model iteration

- Agree the method package and fixed adapter boundary with the owners of the research split and iteration contract. A scheduling policy is not itself a model-generated development method.
- Resolve the open executor and hidden scoring review holds, including Linux isolation for generated code. This macOS checkout does not provide the asserted Linux namespace environment.
- Bind every method run to the durable experiment-call ledger and every model request to the generation/admission/cost chain, including provider-actual usage and unknown outcomes.
- Decide exact model identifier, provider, sampling settings, available credits and a hard maximum spend. Record all three arms and all repeated controls in a pre-run cost estimate. CREAO Trigger credits and Claude API usage must not be treated as interchangeable.
- Freeze generator and protocol revisions, panel commitments, baseline identities, statistical procedure, five-repeat adequacy and stopping rule before any model result can influence them.

At the current local `.env` inspection, `GENESIS_LLM_PROVIDER=anthropic`, `GENESIS_LLM_MODEL=claude-opus-5-5`, and the project API key is present; no key value was copied into evidence. The merged generator currently permits an explicit adaptive reasoning policy only for Sonnet 5.5, so Opus 5.5 would use provider-default reasoning unless the contracts owner adds a reviewed explicit policy. Equal submitted settings can be checked, but hosted defaults or weights cannot be independently proven immutable. The [official Opus 5.5 pricing](https://platform.claude.com/docs/en/models/opus-5-5/overview) is USD $4 per million input tokens and $20 per million output tokens. If all 600 requests matched one earlier retained method revision's 20,798 input and 5,741 output tokens, the illustrative cost would be about USD $119; actual prompts, output, cache treatment and billed balance can differ. This is neither a cost ceiling nor permission to spend it.

On October 25, the user set a **USD $50 total Claude API charge ceiling for this formal study**. This is an authorization limit, not a claim that the planned 600 requests fit within it. Before any paid request, the runner must reserve against that ceiling using the pinned provider price, the request's maximum input/output allowance and all already settled or uncertain usage. It must refuse a request whose maximum possible charge would exceed the remaining budget and stop on unknown billing until reconciled. No formal provider request has been submitted under this ceiling. If the complete predeclared design cannot fit, report a no-go or incomplete study; do not silently reduce rounds, repetitions or controls after observing results.

These gates are concrete engineering work and coordination points. Their absence is a result of the readiness check, not a failed scheduling outcome.

### Readiness decision at the PR #201 review follow-up

**No-go for paid formal iteration yet.** This decision is about missing integration and assurance, not an experimental failure. PR #201's first four independent verifier findings were fixed at `f91952b` and independently cleared before merge. A later pressure test found duplicate-key and overflowing-number acceptance in the offline verifier and a truncated published calibration head; a verifier-integrity follow-up is in progress. The domain-free iteration PR #144 still has a source-binding promotion hold. Hidden scoring #57 and Linux executor isolation #65 remain open, and this macOS host cannot establish the Linux namespace behavior required for generated source. The exploratory strategy/adapter harness #180/#181 is also under lifecycle and host-execution review. The provider-default Opus setting, exact full-chain accounting, hidden panel commitments and enforcement of the user-set USD $50 total charge ceiling remain open. A complete formal run must satisfy these conditions on the exact merged heads and then pass an English pre-run checklist before its first model call.
