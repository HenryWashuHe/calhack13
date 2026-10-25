# Re-execution audit of every retained trial (October 25, 2026)

Henry's Claude, executor/scoring lane. Historical operator report run on main
`58230b71`; the `audit.json` bytes below are unchanged.

**Prospective audit-tool correction:** subsequent static review found that the
original tool could accept old scratch outputs despite a failed command, skip a
malformed retained plan, or succeed with zero audited slots. The corrected tool
requires a new work directory outside the evidence tree, checks subprocess exit
status before accepting outputs, and fails those incomplete-audit cases. Sixteen
synthetic tests (all subprocesses mocked) pass. No methods or scorers were rerun
for this correction. The historical 67/65 result remains the operator's report;
these tests neither independently reproduce nor invalidate that earlier run.

The follow-up boundary correction validates unique slot names as single ASCII
path components (1–128 characters), plus the method/environment digest shapes,
before dispatch. Per-slot scratch paths have separate numeric identities and
must remain inside the new work directory. A retained evaluation with a missing
receipt now fails explicitly, including the single-slot invocation layout.
Declared slots with neither receipt nor evaluation are unexecuted and outside
this retained-execution audit; this is not proof that all declared trials ran.
The three generation-request files also named `plan.json` are recognized by the
complete `MethodSynthesisPlan` wire shape and reported as excluded. They do not
count as audited trials; unknown shapes and any invalid trial `slots` still fail.

## Question

Do the retained records say what re-running them says? An organization that
re-executes and rescores before adopting (#264, #266) depends on this. So does
anyone who wants to trust a past trial without trusting the operator who ran it.

## What ran

`experiments/invention-lab/scoring/tools/reexec_audit.py` over
`docs/invention-lab/evidence/`.

- **Re-execution.** For every slot with a retained execution receipt (trial
  slots and `invoke.py` invocation runs), `lab-exec run` re-ran the slot from
  its retained plan into a fresh scratch directory. It used the slot's method
  package, found by digest in the evidence tree, and the executor build whose
  environment digest the plan names. The fresh receipt had to equal the
  retained one except for measured wall time.
- **Rescoring.** `lab-score score` re-ran each retained evaluation on the
  retained receipt. Where a slot kept no output artifact (the rehearsals), it
  used the re-executed output, which is only allowed once the receipts match.
  The two selective-transfer hidden comparisons were rescored with
  `score-hidden` and the revealed salt.
- **Sandbox.** Namespaces, as root, with CPython 3.14.0 (runtime ref
  `0b21e795…`), on this host only.

The audit writes nothing under the evidence tree. These are reruns, not new
trials: no verdict is read from them, and no retained record changed.

| Environment | Source built | `lab-exec` here | Used by |
| --- | --- | --- | --- |
| `2231a930…` | `635fd3b5` (#65 head, as recorded) | `e7b0ec5d…` | first live receiver, B revision receiver (#116), A return (#117) |
| `90d3591b…` | `6636596` | `4ebbcf22…` | selective-transfer public trial |
| `ab598d6e…` | `4c2648d` | `d9ee77a4…` | selective-transfer hidden attempt 1 (ineligible, kept) |
| `3e168bf6…` | `b59d1eb` | `aaa9dced…` | selective-transfer hidden-2 |
| `276d5465…` | `e2d72468` | `383f0fff…` | rehearsals: invocation, planner chain, two orgs |
| `eea594d4…` | main `58230b71` | `72836d98…` | rehearsals 5 and 6, `g2-attempt-1` |

Scorers: main `lab-score` `5e969c17…` for plain scoring; `130b733a…` (`b59d1eb`)
and `8cfa639d…` (`4c2648d`) for the hidden rescoring. The binary digests here
can differ from those the bundles recorded (for example the rehearsals' `9a112687…`)
because builds are not byte-reproducible. The environment digest the executor
reports is what the plan pins, and it matched in every case.

## Result

**All 67 retained executed slots re-run to identical receipts.** Output digests,
transcript digests, outcomes and simulation steps all matched; only wall time
differs. **All 65 retained evaluations rescore identically.** The two
invocation runs without a retained evaluation are rehearsals that never posted
one.

| Bundle | Environment | Slots | Receipt reproduces | Scoring reproduces | Output re-executed | No evaluation |
| --- | --- | --- | --- | --- | --- | --- |
| g2-attempt-1 | `eea594d4` | 18 | 18/18 | 18 | 0 | 0 |
| live-method-20261024-a-return | `2231a930` | 6 | 6/6 | 6 | 0 | 0 |
| live-method-20261024-b-revision-receiver | `2231a930` | 6 | 6/6 | 6 | 0 | 0 |
| live-method-20261024-receiver | `2231a930` | 5 | 5/5 | 5 | 0 | 0 |
| rehearsal-aba2-receiving-20261025 | `eea594d4` | 9 | 9/9 | 9 | 8 | 0 |
| rehearsal-invocation-20261025 | `276d5465` | 3 | 3/3 | 2 | 2 | 1 |
| rehearsal-live-planner-2-20261025 | `eea594d4` | 2 | 2/2 | 2 | 0 | 0 |
| rehearsal-planner-chain-20261025 | `276d5465` | 3 | 3/3 | 2 | 2 | 1 |
| rehearsal-two-orgs-20261025 | `276d5465` | 3 | 3/3 | 3 | 2 | 0 |
| selective-transfer-20261024 | three builds | 12 | 12/12 | 12 | 0 | 0 |

The per-slot results are in `audit.json`.

## What it means

- Every adoption, rejection and selective-transfer verdict in the retained
  record follows from the method bytes, the frozen plan and the pinned tools,
  as far as re-running can show. Operator receipts weren't silently wrong
  anywhere.
- Determinism holds for every method here, generated and handwritten,
  including seeded random search. That is what makes #266's re-execution check
  usable on this testbed.
- The executor build matters. The current executor refuses the October 24
  plans because its environment digest moved. Re-checking an old trial
  requires keeping, or rebuilding from the recorded source, the build its plan
  names.
- Not covered: generation (model calls are not re-run); wall time and other
  measured usage; cost records; anything that ran outside the executor.
  One host, one operator. This is internal consistency of the retained
  record, not independent-organization evidence.

## Reproduce

Build `lab-exec` from each source above (`experiments/invention-lab/executor`)
and `lab-score` from main and the two hidden-scoring sources
(`experiments/invention-lab/scoring`). With `python3` on `PATH` reporting
exactly `Python 3.14.0`, run as root:

```sh
python3 -I experiments/invention-lab/scoring/tools/reexec_audit.py \
  docs/invention-lab/evidence <work> <lab-score@main> \
  2231a930=<lab-exec@635fd3b5> eea594d4=<lab-exec@main> 276d5465=<lab-exec@e2d72468> \
  90d3591b=<lab-exec@6636596> ab598d6e=<lab-exec@4c2648d> 3e168bf6=<lab-exec@b59d1eb> \
  hidden:selective-transfer-20261024/hidden-2/=<lab-score@b59d1eb> \
  hidden:selective-transfer-20261024/hidden/=<lab-score@4c2648d>
```

Use a previously nonexistent `<work>` path. It exits 0 only if at least one
retained slot is audited and every discovered executed slot reproduces, with no
malformed plan, orphaned retained evaluation or command failure. The historical
operator run took under a minute.
