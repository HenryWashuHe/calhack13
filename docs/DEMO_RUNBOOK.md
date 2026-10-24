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
| Capability need → acquisition | `RequestCapability`, trusted policy and opt-in durable driver merged (#49/#56/#66) | acquisition options and keys configured, **https portal evidence** | `capability_gap_opened` → `…status_changed` (`acquiring`, `acquired`), independent verification proof |
| Verified → published + installed (A) | opt-in finalizer, #78 (+ console #80) | `--gap-finalization-policy` + registry | gap `installed`, `registry:<id>@<version>`, local acceptance runs |
| B adopts before synthesizing | opt-in adopt-first, #82; two-org test #83 | `--gap-adoption-policy` + registry, B's Apify access to A's Actor | B gap `installed` as A's exact version, 0 B model/Actor calls |
| Source generation, Actor build, verification | real code; needs `ANTHROPIC_API_KEY` + `APIFY_TOKEN` | keys + reachable portal | actor/build/run IDs, oracle verdict |
| Venue portal (judge varies it) | real, merged | tunnel up | lab variant, page hashes |
| Org B independent adoption | real code (loopback-proven) | both orgs + registry | B's own acceptance runs, 0 synthesis calls |
| Elastic specialists / planner agent | real code | Elastic keys | smoke report, agent IDs |
| Fetch recruitment | real code; discovery + signed bridge | Agentverse key + bridge | search ID, signed receipts |
| CREAO persistent role | creation/runtime, execution receipts, gateway verification, operator attach, readiness probe and verified persistent mission assessment (#72) merged; optional scenario assessment cycles documented below; live evidence open | CREAO key + configured durable runtime + reachable gateway callback | same agent, two verified assessments, replacement requirement; actual agent ID **and run share link** required by the track |
| Console | real, including Attention panel #28 | always; Attention appears when persisted work needs it | role cards, evidence tables, gap chips and attention items |

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
# --holdout is on main: public synthesis examples are separate from the
# verification pages. Omitting it produces reproducibility checks, not holdout.
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
  --acquisition-policy runtime/acquisition-policy.json --auto-acquire \
  --gap-acquisition-policy runtime/gap-policy.json \
  --gap-finalization-policy runtime/finalize-a.json        # #78
genesis org --org-id org_b --name "Org B" --db data/b.db --bind 127.0.0.1:7102 \
  --registry-url http://127.0.0.1:7100 --registry-token $REG_B \
  --oracle-suite data/portal/trusted/suites/portal-b-v1.json \
  --gap-adoption-policy runtime/adopt-b.json               # #82

# 3. Console and planner
(cd apps/web && npm run dev)                          # http://127.0.0.1:5173
cargo run -p genesis-planner -- run                   # watches Org A
```

Each `genesis org` also needs `GENESIS_OPERATOR_TOKEN`, `GENESIS_AGENT_TOKEN`
and `GENESIS_ADAPTER_TOKEN`; provider keys stay in each server's environment
(`ANTHROPIC_API_KEY`, `APIFY_TOKEN`, `ELASTIC_*`, `CREAO_ACCOUNT_API_KEY`).
Prepare the policy files before starting Org A. Acquisition template:
`crates/genesis-server/examples/acquisition/policy.json`
([SERVER_ACQUISITION.md](SERVER_ACQUISITION.md)). Define `runtime/gap-policy.json`
using [CAPABILITY_GAP_POLICY.md](CAPABILITY_GAP_POLICY.md): each trusted rule needs
the requested capability ID, configured suite ID, exact allowed public URLs and
fixed public HTML examples. The gap's `observed_sources` must include every suite
URL and at least one fixed example URL: materialization narrows the rule to the
requested sources. Keep oracle answers out of examples. The gap driver needs all
three acquisition options above; `--auto-acquire` alone advances existing intents.
Without provider keys, use
`genesis dev` for the console path; it labels every stub `DEV STUB`.

For CREAO readiness, configure the private runtime and shared account quota
journals plus `GENESIS_PUBLIC_URL` as documented in
[CREAO_READINESS.md](CREAO_READINESS.md). Each organization needs its own reachable
callback URL. A provider run ID or completion is insufficient: the matching
gateway receipt is required. This readiness check is separate from persistent
CREAO mission work and its genuine run share link.

### Autonomous capability path (#78, #82)

With both flags above, neither organization needs an operator call for the
capability beat:

1. A's planner commits `RequestCapability`. The gap driver synthesizes and
   verifies (`acquired`); the finalizer publishes that exact pin with
   recomputed proof, installs it and runs A's local acceptance checks
   (`installed`); the kernel note wakes A's planner to attach it.
2. B's planner later commits its own `RequestCapability` (any capability
   name). Adopt-first searches the registry, skips B's own publications,
   installs A's version and accepts it only with B's suite; the gap records
   A's exact version as `installed`. B makes no model or Actor-creation call.

`runtime/finalize-a.json` ([CAPABILITY_GAP_FINALIZER.md](CAPABILITY_GAP_FINALIZER.md)):
one rule per capability ID with A's suite and the publication text.
`runtime/adopt-b.json` ([CAPABILITY_GAP_ADOPTION.md](CAPABILITY_GAP_ADOPTION.md)):
`{"rules":[{"capability_id":"<B's requested ID>","suite_id":"portal-b-v1","synthesis_fallback":false}]}`.

Two honesty points. The adoption policy is B's provider-access
acknowledgement: write it only after B's own Apify credentials can run A's
Actor (same rule as `provider_access_acknowledged` below). And order matters:
B's need must be committed after A's publication, or B's search is empty;
with `synthesis_fallback: false` B's gap is then refused (`no_candidate`;
status at `GET /api/v1/capability-gaps/{id}/adoption`).

Evidence: `tests/two_org_inheritance.rs` runs exactly this with real
`serve_org` on both sides against loopback providers. No live run yet.

Rehearse it without keys: `cargo run -p genesis-server --example
rehearse_autonomy` starts the registry and both orgs on the console's default
ports with a slowed fixture provider (`REHEARSAL_DELAY_MS`), then runs A's and
B's requests in order while the console shows the chips move. Every external
effect is a fixture; it is for timing and narration practice only, never a
recording to present as live.

## The 3 minutes

Targets from the research report §10, with what each beat actually shows today.

| Time | Beat | Show | Status |
| --- | --- | --- | --- |
| 0:00–0:20 | Mission, starting org, budget | console org tree | ready |
| 0:20–0:40 | Judge closes the venue / changes the portal in the lab | lab → new namespace; observation with `evidence_ref` = portal URL | ready |
| 0:40–1:15 | Planner: new requirement → CreateRole / AssignResponsibility / **RequestCapability**; Elastic specialist; Fetch offer | decision event, gap chip REQUESTED (#54) | planner, gap chip, lifecycle wake-up and opt-in driver merged (#49/#54/#57/#66) |
| 1:15–1:45 | Acquisition: generated parser, pinned build, oracle checks pass: **held out** only if A was generated with `--holdout` (#60); otherwise call them reproducibility checks, since A's examples would be suite cases | gap ACQUIRING → ACQUIRED → INSTALLED; actor/build/run IDs, `registry:` ref | needs keys + trusted policy; INSTALLED needs the finalizer (#78) |
| 1:45–2:10 | Second disruption (capacity cut) handled by the existing owner, **no new role** | distinct frozen capacity-cut source; one subscription, same CREAO monitor, accepted reassessment and linked replacement requirement | bounded two-cycle scenario; native run evidence pending |
| 2:10–2:40 | Org B: own requirement, discovers A's capability, own acceptance, different input | B gap INSTALLED as A's version, B's acceptance runs, 0 synthesis calls | operator scenario ready; no-operator path needs #82 (loopback-proven, #83) |
| 2:40–3:00 | Temporary role retires after handoff; capability remains | retirement event, stale epoch fenced | ready |

The planner doc's best evidence of reasoning: two disruptions, two different
organizational responses. The portal's `layout-drift` preset is the honest
way to force a repair: the hand-written baseline parser matches the oracle
3/3 on baseline variants and 0/3 on layout drift.

## Operator-directed full scenario

The existing scenario coordinates acquisition, publication and independent
adoption under an operator request. Label that path explicitly. Without the
finalizer and adopt-first flags, the automatic `RequestCapability` →
policy-driver path stops at verified `Acquired`.

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

For the persistent CREAO core-work demonstration, use the
[monitoring setup walkthrough](VENUE_PORTAL.md#prepare-continuing-creao-monitoring)
before starting either server. It generates three immutable namespaces and uses
`prepare-monitoring.mjs` to assemble the frozen B suite and complete scenario spec,
including the public source allowlist and explicit event policy. Start B with
`--oracle-suite runtime/monitoring-001/trusted/suite-b.json` and run the scenario
with `--spec runtime/monitoring-001/scenario.json`. The simpler fragment-combining
command above does not configure monitoring. Configure B's CREAO
creation/runtime/readiness path; development-role opt-in cannot stand in for this
monitor. The driver registers one subscription for both cycles, then observes its
children without directly creating assessments. The
[scenario report](INHERITED_MISSION.md#continuing-creao-mission-work) identifies the
subscription, two native runs and their accepted mission effects, but a live
account run and genuine Share link remain separate evidence to capture. A timeout
or ambiguous registration does not make fresh-scenario replay safe: bounded server
work can continue, and an operator must inspect or reconcile it.

Checked: with generated fragments, this merge and `true` pass the CLI's spec
validation (it stops only at the unreachable org URLs). Keeping `note` fails
decoding; `false` gives the acknowledgment error above.

## After the run: audit both organizations

```bash
genesis audit --db data/dev/org_a.db --org-id org_a
genesis audit --db data/dev/org_b.db --org-id org_b
```

Start the stack with `GENESIS_AUDIT_COMMITS=1` so the kernel also refuses any
commit that would break an invariant while it runs. The audit itself is
read-only. It checks every stored snapshot, every consecutive pair and every
event against the kernel invariants (limits, reporting tree, single active
assignment, append-only history, legal lifecycle moves, installed gaps backed
by accepted installations; full list in [KERNEL_AUDIT.md](KERNEL_AUDIT.md)) and
exits non-zero on any violation. A clean dev-scenario run: Org A 11 snapshots
and 67 events, Org B 6 and 23, 0 violations. Use it as the closing line:
"every state this organization ever held is still well formed."

## If something fails on stage

| Symptom | Say / do |
| --- | --- |
| Cold build too slow | Switch to the labeled recording of a full run; keep the live run going in the background |
| `needs_reconciliation` anywhere | That is the safety property: GENESIS refuses to guess. Show the event log entry and the Attention panel |
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
