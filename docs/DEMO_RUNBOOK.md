# Demo runbook

One place for demo day: what to start, in what order, what each step proves,
and what is live, fixture or still pending. Every command here exists in the
repository; anything not yet on `main` names its PR.

Cal Hacks 13.0: submission **Oct 25, 9:00 AM PT** (edits until 10:00; table
number locks at 9:00). Pitch is 3 minutes plus 2 minutes of Q&A, with a
locally stored recording.

## Honesty table

Say which path is shown. A recorded full run is fine if labeled as one; a
prebuilt artifact must never be presented as freshly generated.

| Piece | Status on `main` | Live when | Proof to show |
| --- | --- | --- | --- |
| Kernel: changes, saga, fencing, retirement | real | always | event log, revisions, stale-epoch rejection |
| Planner proposes changes | real code; model needs `GENESIS_LLM_*` or `GENESIS_LLM_PROVIDER=elastic` | model key set | decision events, kernel rejections fed back |
| Capability need → acquisition | `RequestCapability` merged (#49); installed → planner wake-up and https source rule in #57; policy materializer #56; driver worker pending (Codex) | #57 + driver merged, **https portal evidence** | `capability_gap_opened` → `…status_changed` → `observation_recorded` (`kernel.capability_gap`) → `AttachCapability` |
| Source generation, Actor build, verification | real code; needs `ANTHROPIC_API_KEY` + `APIFY_TOKEN` | keys + reachable portal | actor/build/run IDs, oracle verdict |
| Venue portal (judge varies it) | real, merged | tunnel up | lab variant, page hashes |
| Org B independent adoption | real code (loopback-proven) | both orgs + registry | B's own acceptance runs, 0 synthesis calls |
| Elastic specialists / planner agent | real code | Elastic keys | smoke report, agent IDs |
| Fetch recruitment | real code; discovery + signed bridge | Agentverse key + bridge | search ID, signed receipts |
| CREAO persistent role | creation/readiness code; **core mission work still open** (CREAO_SPONSOR_NOTES) | CREAO key | agent ID **and run share link** required by the track |
| Console | real | always | role cards, evidence tables, attention panel |

## T−24 h: live gates first

```sh
# Every binary the runbook calls: genesis (server, registry, scenario),
# genesis-planner, genesis-forge, genesis-creao, genesis-fetch and
# genesis-elastic-smoke. Then put them on PATH for this shell.
cargo build --bins -p genesis-server -p genesis-planner -p genesis-forge \
  -p genesis-creao -p genesis-fetch -p genesis-elastic
export PATH="$PWD/target/debug:$PATH"
set -a; source .env; set +a
node scripts/live-gates/run.mjs                       # Elastic, Fetch (free)
node scripts/live-gates/run.mjs --allow-billable \
  --portal-suite data/portal/trusted/suites/<suite>.json   # + Apify, CREAO
```

[LIVE_GATES.md](LIVE_GATES.md) (#47). Paste `data/live-gates/gates-*.md` into
issue #4. Record cold Actor build time: it decides whether the 3-minute demo
runs cold or uses a labeled recording.

## Start the stack

Each terminal needs the binaries on PATH: `export PATH="$PWD/target/debug:$PATH"`
from the repository root, after the build above.

```sh
# 0. Local, untracked working files (scenario spec, fragments, gap policy)
mkdir -p runtime

# 1. Public portal (no account): get the URL first, then generate, then serve
cloudflared tunnel --url http://127.0.0.1:7200
U=https://<random>.trycloudflare.com
# --holdout needs #60; until it is on main, drop the flag (checks are then
# reproducibility checks, not held out).
node scripts/venue-portal/generate.mjs --preset baseline --seed a1 --namespace org-a \
  --suite-id portal-a-v1 --base-url $U --out data/portal --holdout
node scripts/venue-portal/generate.mjs --preset baseline --seed b1 --namespace org-b \
  --suite-id portal-b-v1 --attendees 150 --base-url $U --out data/portal
node scripts/venue-portal/serve.mjs --out data/portal --base-url $U       # :7200, lab :7201

# Capability path: observations must cite the https tunnel URL, not
# http://127.0.0.1:7200. Sources must be canonical https (#57) and the gap
# policy allowlist (#56) matches exact strings, so list the exact pages, e.g.
#   https://<random>.trycloudflare.com/layout-drift/venues/main-hall.html
# A new tunnel URL means regenerating pages, suites and that allowlist.

# 2. Registry and two fresh organizations (separate databases and tokens)
genesis registry --publishers org_a:$REG_A,org_b:$REG_B
genesis org --org-id org_a --name "Org A" --db data/a.db --bind 127.0.0.1:7101 \
  --registry-url http://127.0.0.1:7100 --registry-token $REG_A \
  --oracle-suite data/portal/trusted/suites/portal-a-v1.json \
  --acquisition-policy runtime/acquisition-policy.json --auto-acquire
genesis org --org-id org_b --name "Org B" --db data/b.db --bind 127.0.0.1:7102 \
  --registry-url http://127.0.0.1:7100 --registry-token $REG_B \
  --oracle-suite data/portal/trusted/suites/portal-b-v1.json

# 3. Console and planner
(cd apps/web && npm run dev)                          # http://127.0.0.1:5173
cargo run -p genesis-planner -- run                   # watches Org A
```

Each `genesis org` also needs `GENESIS_OPERATOR_TOKEN`, `GENESIS_AGENT_TOKEN`
and `GENESIS_ADAPTER_TOKEN`; provider keys stay in each server's environment
(`ANTHROPIC_API_KEY`, `APIFY_TOKEN`, `ELASTIC_*`, `CREAO_ACCOUNT_API_KEY`).
Policy template: `crates/genesis-server/examples/acquisition/policy.json`
([SERVER_ACQUISITION.md](SERVER_ACQUISITION.md)). Without provider keys, use
`genesis dev` for the console path; it labels every stub `DEV STUB`.

## The 3 minutes

Targets from the research report §10, with what each beat actually shows today.

| Time | Beat | Show | Status |
| --- | --- | --- | --- |
| 0:00–0:20 | Mission, starting org, budget | console org tree | ready |
| 0:20–0:40 | Judge closes the venue / changes the portal in the lab | lab → new namespace; observation with `evidence_ref` = portal URL | ready |
| 0:40–1:15 | Planner: new requirement → CreateRole / AssignResponsibility / **RequestCapability**; Elastic specialist; Fetch offer | decision event, gap chip REQUESTED (#54) | planner ready; gap + chip merged (#49, #54); wake-up #57; driver pending |
| 1:15–1:45 | Acquisition: generated parser, pinned build, oracle checks pass: **held out** only if A was generated with `--holdout` (#60); otherwise call them reproducibility checks, since A's examples would be suite cases | gap ACQUIRING → VERIFIED → INSTALLED; actor/build/run IDs | needs keys + driver; else `genesis scenario` labeled path |
| 1:45–2:10 | Second disruption (capacity cut) handled by the existing owner, **no new role** | lab `capacity-cut` namespace; planner `no_change` or reassignment | ready (planner); CREAO second run pending |
| 2:10–2:40 | Org B: own requirement, discovers A's capability, own acceptance, different input | B adoption intent, B's acceptance runs, 0 synthesis calls | ready (loopback-proven) |
| 2:40–3:00 | Temporary role retires after handoff; capability remains | retirement event, stale epoch fenced | ready |

The planner doc's best evidence of reasoning: two disruptions, two different
organizational responses. The portal's `layout-drift` preset is the honest
way to force a repair: the hand-written baseline parser matches the oracle
3/3 on baseline variants and 0/3 on layout drift.

## Until the gap driver lands

Run the capability beat with the existing operator-directed scenario and say
so: "the operator approves acquisition here; the autonomous path is
`RequestCapability` → policy driver."

```sh
GENESIS_ORG_A_OPERATOR_TOKEN=… GENESIS_ORG_B_OPERATOR_TOKEN=… \
genesis scenario --spec runtime/mission.json --org-a http://127.0.0.1:7101 --org-b http://127.0.0.1:7102
```

Build `runtime/mission.json` from the checked-in sample plus both portal
fragments. The scenario input is strict, so drop the fragment's
informational `note`:

```sh
mkdir -p runtime
node scripts/venue-portal/combine-fragments.mjs \
  --a data/portal/fragments/org-a.json --b data/portal/fragments/org-b.json > runtime/fragment.json
node -e 'const fs=require("fs");const [m,f,o]=process.argv.slice(1);
  const {note,...frag}=JSON.parse(fs.readFileSync(f,"utf8"));
  fs.writeFileSync(o,JSON.stringify({...JSON.parse(fs.readFileSync(m,"utf8")),...frag},null,2)+"\n")' \
  crates/genesis-server/examples/scenario/mission.json runtime/fragment.json runtime/mission.json
```

`provider_access_acknowledged` stays `false`, and the CLI refuses to start
("permitted shared Actor access must be acknowledged"), until B's own,
separately managed Apify credentials are actually authorized to run A's
Actor. Arrange that first, then set it to `true` in `runtime/mission.json`.
Registry publication grants no provider access
([INHERITED_MISSION.md](INHERITED_MISSION.md), [VENUE_PORTAL.md](VENUE_PORTAL.md)).

Checked: with generated fragments, this merge and `true` pass the CLI's spec
validation (it stops only at the unreachable org URLs). Keeping `note` fails
decoding; `false` gives the acknowledgment error above.

## If something fails on stage

| Symptom | Say / do |
| --- | --- |
| Cold build too slow | Switch to the labeled recording of a full run; keep the live run going in the background |
| `needs_reconciliation` anywhere | That is the safety property: GENESIS refuses to guess. Show the attention panel |
| Tunnel URL changed | Regenerate into a **new** namespace for the new URL; never replace one a run used |
| Planner rejected | Show the kernel's exact errors fed back and the repaired proposal |
| A sponsor API is down | Fall back to that sponsor's recorded gate evidence (`data/live-gates`), labeled |

## Open human decisions

1. Cal Hacks eligibility: past Cal Hacks rules say the hack must be developed
   entirely during the event; most of this repository predates Oct 23.
   Confirm with organizers and decide how to disclose.
2. Which model runs the planner (`GENESIS_LLM_*` or Elastic).
3. Sponsor keys for the four live gates, and who runs them.
4. CREAO track: core mission work on a real agent, its agent ID and run share
   link in the Devpost README, shown running in the demo.
