# CREAO team API Trigger smoke

This is a separate smoke path for an agent made in
`agent.creao.ai`. The current `genesis-creao` runtime and the CREAO gate in
`scripts/live-gates/run.mjs` use the Developer API. The app API Trigger uses a
`capi_` key, an app ID in the URL, and an `inputs` body. Do not alias a Developer
Platform `cr_sk_` key into this path. See the [official API Trigger contract](https://docs.creao.ai/pro/api-trigger)
and [API path comparison](https://docs.creao.ai/developer-api/api-paths).

## Before a credit-consuming call

1. In the intended CREAO team, create and publish the agent that will do core
   GENESIS work. Copy its exact UUID app ID from its detail page. The API Trigger
   key must be authorized for that app; [CREAO documents app-scoped keys](https://docs.creao.ai/pro/api-trigger#app-scoped-api-trigger).
   A key's `capi_` prefix alone does not establish its app scope.
2. Check the intended team's **Plan & billing → Credits** balance and record it
   privately. The official API Trigger docs say runs consume CREAO account
   credits; the account UI, not this script, determines which balance actually
   changes. This script makes no claim that an untested key will debit a
   particular 600-credit subscription grant.
3. Put only the full `capi_` value in the private environment as
   `CREAO_TRIGGER_API_KEY`. Do not put it in a command argument, input file,
   issue, or report. Write a private JSON object with the agent's actual input
   fields, for example `{ "task": "Assess this synthetic venue update" }`.
   A placeholder input does not demonstrate core mission work.

Offline preflight checks the app ID, input JSON and key prefix. It sends no
request and creates no journal:

```sh
node scripts/live-gates/creao-trigger.mjs \
  --app-id <agent-app-uuid> --inputs <private-input.json> \
  --out data/live-gates/creao-trigger/<unique-run>.json
```

After explicit authorization for **one** credit-consuming run, use the same
arguments with `--allow-credits`. The command journals intent before making a
single `POST /api/v1/apps/{appId}/runs`, then polls only the returned run ID via
`GET`. A completed or uncertain journal can never issue a second POST. Reusing
the exact output path after a temporary GET failure resumes GET only.
The output directory must already exist on storage that supports directory
`fsync`; the CLI refuses to create it during a credit-consuming call. It syncs
the journal file and directory entry before POST and synchronizes replacements.
Only one process may use a journal at a time. If a process crashes and leaves a
`.lock` file, check that no process is still using the journal and reconcile its
record with CREAO before manually clearing the lock.

```sh
node scripts/live-gates/creao-trigger.mjs \
  --app-id <agent-app-uuid> --inputs <private-input.json> \
  --out data/live-gates/creao-trigger/<unique-run>.json --allow-credits
```

The JSON report retains the app/run IDs, status, exact input hash, output hash,
and any `usage.credits` number CREAO returns. It does not retain the key or the
agent's result text. A successful terminal status is **one live agent run**, not
proof of the agent's domain correctness, a share link, CREAO track eligibility,
or a fixed-weight A → B → A improvement. Independently inspect the result in
CREAO and record the genuine share link if needed for sponsor evidence.

`needs_reconciliation` means the POST might have reached CREAO without an exact
run ID. Do not choose a fresh output path and retry blindly. Inspect the app's
run history for the attempted time, then handle the old journal as a distinct
operation. The script has no automatic POST replay or provider-output override.
Before a second test, check the team's Credits page again to verify the actual
credit source and amount charged. On October 24, one explicitly authorized
synthetic API Trigger run completed; its status was confirmed through the same
journal and the CREAO page, and its debit was checked privately in the team
billing view. This establishes a live API Trigger path only. It does not verify
the generated revision, close a GENESIS gate, or establish a recursive benefit.
The repository tests use injected offline responses only.
