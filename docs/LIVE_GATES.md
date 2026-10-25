# Live sponsor gates

[TEAM_WORKSTREAMS.md](TEAM_WORKSTREAMS.md) originally made the four live API
gates the first deliverable. This kit runs each gate with the repository's own
durable CLIs and the shared CREAO Trigger journal, then writes sanitized evidence. Reported partial live
results and remaining limits are recorded below; local tests remain separate.

```sh
cargo build -p genesis-forge -p genesis-fetch     # + genesis-elastic-smoke from the Elastic branch
set -a; source .env; set +a                                         # keys stay in the environment
node scripts/live-gates/run.mjs                                     # free gates only: Elastic, Fetch
node scripts/live-gates/run.mjs --allow-billable \
  --portal-suite data/portal/trusted/suites/<suite>.json            # + Apify build/runs, CREAO app run
```

Output: `data/live-gates/gates-<run>.json` (full sanitized record) and
`gates-<run>.md` (a table to paste into current issue #1 or the Devpost write-up). The
process exits 1 if any gate failed or needs reconciliation. An explicit
terminal failure (Apify `rejected`/`failed`, CREAO `failed`) is `failed` even
when earlier build or run IDs exist; `partial` means unfinished evidence.

Each run works in its own directory, `data/live-gates/apify/<run>/` and
`data/live-gates/creao/<run>/`, so earlier checkpoints are never touched.
CREAO uses `data/live-gates/creao/<run>/trigger.json`: the shared
`runTrigger` helper journals intent before its one app-scoped POST and stores
only IDs, hashes, the structured check outcome and provider-reported credits.
The old Developer API agent-creation quota database is not used by this gate.

For a run that stopped, the result records the next step as `resume` (a
shell-quoted command using the absolute binary path) and `resume_argv` (the
same thing as an array). Apify includes `--retry-read` only when the CLI's own
last record explicitly allows it. CREAO resumes this runner with the same
`--run-id`, app, exact synthetic inputs and validation contract: accepted runs
continue GET polling only. An ambiguous POST requires reconciliation and is
never replayed. A different run ID deliberately creates a different run and can
consume credits again; it is not a retry. Never rerun `prepare` for a stopped
Apify operation.

| Gate | Credentials | What runs | Evidence recorded | Pass means |
| --- | --- | --- | --- | --- |
| Elastic | `ELASTICSEARCH_URL`, `ELASTIC_DATA_API_KEY`, `ELASTIC_KIBANA_URL`, `ELASTIC_MANAGEMENT_API_KEY` | `genesis-elastic-smoke --skip-capabilities --converse --cleanup` | agent ID, reconcile ID, evidence doc, conversation ID | every smoke step passed |
| Fetch | `AGENTVERSE_API_KEY` | `genesis-fetch discover` | search ID, hit count, compatible addresses | reported as **partial**: authenticated discovery only |
| Apify | `APIFY_TOKEN` + `--allow-billable` | `genesis-forge prepare` + one `step` at a time | actor, build, build number, run, dataset and failure phase | phase `verified` (oracle-checked runs) |
| CREAO | `CREAO_TRIGGER_API_KEY`, `CREAO_TRIGGER_APP_ID` + `--allow-billable` | shared app-scoped `runTrigger` POST, then GET polling | existing app ID, run ID, credits or explicit unknown, input/result hashes, check outcome and reason | completed run whose synthetic role/workflow response matches the declared local-decline check |

CREAO uses the existing published team agent on
`https://agent.creao.ai/api/v1/apps/<app-id>/runs`, authenticated by its `capi_`
Trigger key. Optional `CREAO_TRIGGER_RUNS_URL` must exactly equal that URL for
`CREAO_TRIGGER_APP_ID`: other hosts/apps, userinfo, query strings, fragments,
ports and path variations are refused before any call. `CREAO_API_VERSION` may
be unset, blank or exactly `trigger`; other explicit values are refused before
any request or journal creation. No `CREAO_ACCOUNT_API_KEY` or `CREAO_V2_API_KEY`
fallback is used. Those historical
keys and rotated Trigger keys remain covered by evidence redaction. See the
[official Trigger contract](https://docs.creao.ai/pro/api-trigger) and
[route comparison](https://docs.creao.ai/developer-api/api-paths).

The benign synthetic input asks the persistent design-review role to assess a
candidate with 2/6 local successes against its 4/6 control, retaining the existing
owner and declining transfer. It forbids browsing, external actions and code
execution. A completed native run is insufficient: the returned structured
JSON must identify the declared responsibility and owner, preserve the scores,
and choose `decline`. The retained check has a versioned identity; changing it
requires a new contract ID. Raw model output is hashed, not saved by the helper.

A passed gate establishes this limited synthetic response check. It does not
create an agent or prove gateway readiness, core mission correctness, actual
organizational adoption, a share link or sponsor qualification. `usage.credits`
is recorded only when the provider returns a finite nonnegative number;
otherwise `usage_reason=not_reported_by_provider` explicitly preserves the gap.
The [standalone Trigger smoke](CREAO_TRIGGER_SMOKE.md) uses the same durable
transport but does not automatically apply this gate's role/workflow check.

A gate without required credentials (or a required binary for the other gates)
is `not_tested`. CREAO needs no `genesis-creao` binary. Without `--allow-billable`
it performs no request; with the flag it consumes CREAO account credits for one
run of the configured existing app. Apify spends build and run credits.

Historical migration note: the former gate created a Developer API agent and
labeled `native_completed` a pass. Its creation/V2-specific tests were deliberately
renamed for app-run identity, GET-only resume, terminal failure and separate
journals. Generic credential, consent, Apify, Fetch and Elastic regressions remain.
Historical live reports below keep their original protocol and do not validate
this migrated gate.

## Reported live checkpoint, October 24, 2026

With owner consent recorded in [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1),
the first Apify rerun after #21 created a private Actor and a successful build;
its case run failed because the checked-in `fixtures.genesis.example` pages do
not resolve. A later run with a tunneled venue portal produced a successful
build and `SUCCEEDED` case run. The dataset later contained one item whose
venue fields matched the trusted suite's expected values in a direct offline
comparison that excluded per-venue evidence, but the
forge's immediate collection returned `InvalidDataset` and credited 0/3 cases.
The gate is **failed**, not verified; see the
[exact issue report](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6380843098).
The private cloud checkpoints remain local to the reporting machine and were
not independently inspected for this documentation update.

[PR #26](https://github.com/HenryWashuHe/genesis-history-experiment/pull/26)
merged at `f153f58`, adding a bounded reread of an empty dataset page after a
successful run. Independent cases are preserved by #29 (`b69f2ba`).
Its passing loopback checks do not confirm the cause of the live failure or a
successful forge rerun. A [later rerun after #26](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6381127464)
still failed collection: its report places the final read about 1.3 seconds after
run completion, while the item was readable later. #34, merged at `0b4c18b`,
decouples dataset spacing from Actor polling: sixteen reads at one-second waits,
within the original deadline. Its new sixth-read regression reproduces the old
cutoff and passes after correction; all Apify/forge suites pass locally. No new
live validation confirms this wider-window mitigation. CREAO v2 metadata reads authenticated but returned no
hosted agents; [PR #25](https://github.com/HenryWashuHe/genesis-history-experiment/pull/25)
merged at `0d6d96d`, adding opt-in v2 handling and protocol-correct receipt
hashing with offline checks. A live v2 agent/run is still unproven. Fetch discovery has partial authenticated evidence; Elastic's data
index permission issue is still pending.

## Apify needs reachable pages

The checked-in suite points at `fixtures.genesis.example`, which does not
resolve. Without a reachable `--portal-suite`, Actor creation and build may
succeed but attempted case execution fails; a terminal failure remains **failed**.
For case runs, host the
[venue portal](VENUE_PORTAL.md) behind a tunnel and pass its suite:

```sh
cloudflared tunnel --url http://127.0.0.1:7200      # note the https URL
node scripts/venue-portal/generate.mjs --preset baseline --seed gate --namespace gate \
  --base-url https://<random>.trycloudflare.com --out data/portal
node scripts/venue-portal/serve.mjs --out data/portal --base-url https://<random>.trycloudflare.com
```

The gate's parser is the hand-authored baseline
(`actors/venue-extractor-template/tests/baseline-extractor.ts`), **not AI
synthesis**. Pre-flight, run locally with Node type stripping: it matches the
oracle exactly on 3/3 cases for four independent `baseline` seeds and 0/3 for
`layout-drift`. So the baseline preset is a fair live check of Apify itself,
and layout drift is the variant that should force a semantic repair.

## Safety

The kit uses the existing CLIs and the shared Trigger helper. It never edits
a checkpoint or replays a POST. Apify passes `--retry-read` only when the last record reports a retryable,
non-ambiguous read (Forge reports that with exit 1, which does not stop the
gate), and stops at the first unknown outcome (`needs_reconciliation`). Resume
or reconcile with the CLI named in the result. Stdout is parsed into
allow-listed fields; credential values are scrubbed from everything written.
The Apify checkpoint holds private source and outputs and stays local.

## Not covered

- Fetch's typed offer/accept/task/result round trip:
  `scripts/demo-fetch-recruitment.py` (see [FETCH_RUNTIME.md](FETCH_RUNTIME.md)).
- CREAO gateway skill and readiness receipt: [CREAO_READINESS.md](CREAO_READINESS.md).
  Native completion is not readiness.
- Elastic's shared capability index write (`--skip-capabilities`).
- Anthropic model synthesis, which the acquisition path uses.

Tests: `node --test scripts/live-gates/*.test.mjs` uses stub CLIs, injected
fetch responses and loopback fixtures only. Coverage includes strict Trigger
URL/key admission, credential and billable preflight, schema-checked synthetic
results, retained credits/unknown usage, no duplicate POST after ambiguity,
GET-only resume, current/previous-key redaction, stream limits and concurrent
journal locking, plus the retained Apify/Fetch/Elastic and review47 regressions.
Passing these checks is not evidence of a live provider run.
