# Live sponsor gates

[TEAM_WORKSTREAMS.md](TEAM_WORKSTREAMS.md) makes the four live API gates the
first deliverable (hours 0–4). Until they run, every sponsor claim rests on
loopback fixtures. This kit runs each gate with the repository's own durable
CLIs and writes one sanitized evidence file.

```sh
cargo build -p genesis-forge -p genesis-creao -p genesis-fetch     # + genesis-elastic-smoke from the Elastic branch
set -a; source .env; set +a                                         # keys stay in the environment
node scripts/live-gates/run.mjs                                     # free gates only: Elastic, Fetch
node scripts/live-gates/run.mjs --allow-billable \
  --portal-suite data/portal/trusted/suites/<suite>.json            # + Apify build/runs, CREAO agent
```

Output: `data/live-gates/gates-<run>.json` (full sanitized record) and
`gates-<run>.md` (a table to paste into issue #4 or the Devpost write-up). The
process exits 1 if any gate failed or needs reconciliation.

| Gate | Credentials | What runs | Evidence recorded | Pass means |
| --- | --- | --- | --- | --- |
| Elastic | `ELASTICSEARCH_URL`, `ELASTIC_DATA_API_KEY`, `ELASTIC_KIBANA_URL`, `ELASTIC_MANAGEMENT_API_KEY` | `genesis-elastic-smoke --skip-capabilities --converse --cleanup` | agent ID, reconcile ID, evidence doc, conversation ID | every smoke step passed |
| Fetch | `AGENTVERSE_API_KEY` | `genesis-fetch discover` | search ID, hit count, compatible addresses | reported as **partial**: authenticated discovery only |
| Apify | `APIFY_TOKEN` + `--allow-billable` | `genesis-forge prepare` + one `step` at a time | actor, build, build number, failed run | phase `verified` (oracle-checked runs) |
| CREAO | `CREAO_ACCOUNT_API_KEY` + `--allow-billable` | `genesis-creao prepare` + one `step` at a time | operation, run ID, agent ID | phase `native_completed` |

A gate without credentials or its binary is `not_tested`, never a pass.
Billable gates need `--allow-billable`: CREAO creates a real agent with no
delete endpoint, and Apify spends build and run credits.

## Apify needs reachable pages

The checked-in suite points at `fixtures.genesis.example`, which does not
resolve. Without `--portal-suite` the Apify gate can at best show a created
Actor and a build (**partial**). For case runs, host the
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

The kit only drives the CLIs. It never edits a checkpoint, never replays a
POST, passes `--retry-read` only when the last record reports a retryable
read, and stops at the first unknown outcome (`needs_reconciliation`). Resume
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

Tests: `node --test scripts/live-gates/gates.test.mjs` drives stub CLIs
(no network): missing credentials, billable consent, evidence parsing and
scrubbing, CREAO polling with an explicit read retry, Apify stopping at an
unknown outcome, and build-only versus verified.
