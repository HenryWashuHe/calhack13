# Flood watch: a GENESIS organization that responds to real flood data

**Status: experiment.** A separate scenario next to the invention-lab work. It
changes no shared files and no kernel code.

A riverside summer camp on the Guadalupe River in Texas runs as a GENESIS
organization. It watches the official river gauge and National Weather Service
alerts. When the river floods, the organization changes shape. It creates a flood
response role, escalates without restructuring when the river keeps rising, and
retires that role once the threat ends, handing ongoing watching to a monitor.
Then the next town downstream (Kerrville) installs the camp's verified gauge
reader instead of building its own.

## Why this data

Two free government feeds, no account or API key:

| Feed | What it gives us |
| --- | --- |
| [NOAA National Water Prediction Service](https://api.water.noaa.gov/nwps/v1/docs/) (`/gauges/{lid}`) | Current river stage, the gauge's **official** flood stages (action / minor / moderate / major), and the next gauge downstream |
| [National Weather Service alerts](https://www.weather.gov/documentation/services-web-api) (`/alerts/active?point=lat,lon`) | Active Flash Flood / Flood Warnings and Watches at the gauge's location |

The default gauge is `HNTT2`, Guadalupe River at Hunt. Its downstream neighbors
are `GRHT2` and `KRRT2` (Kerrville), the stretch hit by the July 2025 floods.

## How it fits with the rest of GENESIS

| Flood story | GENESIS mechanism | Where |
| --- | --- | --- |
| Gauge or alert changes | Observation with an `https` evidence reference | `POST /missions/{id}/observations` |
| "Respond to the flood" | Requirement citing that observation | `POST /missions/{id}/requirements` |
| Flood response lead appears | `CreateRole` + `AssignResponsibility`, validated by the kernel against `constitution.json` | `POST /org-changes` |
| River rises again | Job for the **existing** role; no new role | `POST /jobs` |
| Threat ends | `CreateRole` River monitor + `RetireRole` with handoff | `POST /org-changes` |
| Kerrville reuses the reader | Publish → registry search → install (0 synthesis calls) → local acceptance → `AttachCapability` | capability routes |

`flood.mjs` holds the pure logic: fetch, classify, detect changes and build
observations. `flood-watch.mjs` is the driver, `constitution.json` defines the
organization's allowed scopes, and `fixtures/hunt-rising.json` is a rehearsal
timeline.

## What is real and what is not

| Piece | Status |
| --- | --- |
| Kernel validation, revisions, retirement, registry, install | Real GENESIS server, unmodified |
| Live gauge and alert reading (`--gauge`) | Real public data. Checked against both feeds on Oct 24, 2026 |
| `fixtures/hunt-rising.json` | **Synthetic.** Real HNTT2 flood stages, invented readings and alerts. Observations say `flood-watch-fixture` |
| Org changes with `--scripted` | **Scripted proposals**, not the planner. The kernel still validates each one. Without `--scripted`, only observations and the requirement are posted, and a running planner decides |
| Provider bindings (Elastic, CREAO) | Dev stubs (`--dev-stub`), labeled in the console |
| Published capability | A local Node module identified by its sha256, not an Apify Actor. Its verification is a self-check: two success cases plus one missing-data case, as the kernel requires |

Missing or unreadable data is reported as `unknown`, never as safe. This is an
extra layer of awareness, **not** a replacement for official warnings or 911.

## Demo (about 2 minutes)

From the repository root, with Rust 1.94+ and Node 22+. Use one terminal per
line:

```sh
cargo build -p genesis-server --bin genesis
export C=experiments/flood-watch/constitution.json R=http://127.0.0.1:7100
target/debug/genesis registry --db data/flood/registry.db --publishers org_a:org_a-registry,org_b:org_b-registry
target/debug/genesis org --org-id org_a --name "Org A · Hunt river camp" --db data/flood/org_a.db --bind 127.0.0.1:7101 --constitution $C --registry-url $R --registry-token org_a-registry --dev-stub
target/debug/genesis org --org-id org_b --name "Org B · Kerrville" --db data/flood/org_b.db --bind 127.0.0.1:7102 --constitution $C --registry-url $R --registry-token org_b-registry --dev-stub
cd apps/web && npm install && npm run dev        # console at http://127.0.0.1:5173
```

Then run the flood:

```sh
node experiments/flood-watch/flood-watch.mjs --replay experiments/flood-watch/fixtures/hunt-rising.json \
  --genesis http://127.0.0.1:7101 --scripted --org-b http://127.0.0.1:7102
```

What to point at in the console:

1. **Flash Flood Warning, 10.8 ft (minor stage 10 ft):** the observation appears and **Flood response lead** joins Org A's tree.
2. **13.9 ft (moderate):** the same lead escalates. No new role.
3. **River back to 7.9 ft, no alerts:** the lead retires, and **River monitor** inherits the responsibility.
4. **Switch to Org B:** River operations has `cap_river_flood_stage@0.1.0` attached, and the script printed `synthesis calls: 0`.
5. **Audit:** `target/debug/genesis audit --db data/flood/org_a.db --org-id org_a` should report `violation_count: 0`.

Delete `data/flood/` for a clean rerun.

Watching live, which prints the real river right now:

```sh
node experiments/flood-watch/flood-watch.mjs --once
node experiments/flood-watch/flood-watch.mjs --gauge KRRT2 --interval 300 --genesis http://127.0.0.1:7101
```

## Checks

```sh
node --test experiments/flood-watch/flood.test.mjs
```

The 7 tests cover classification, warnings vs. watches, unknown data,
transitions, the fixture timeline, observation labeling and the exact live URLs.
They make no network calls.

Verified end to end on Oct 24, 2026, against the unmodified server: Org A went
from r0 to r7, B went from r0 to r5 with 0 synthesis calls, and the audit
checked 8 snapshots and 40 events with 0 violations.

## Next steps

- Let the planner propose the flood role from the live observation, with a model key, instead of `--scripted`.
- Replay a real past flood from USGS archived readings rather than a synthetic one.
- Add notification and shelter-lookup capabilities, so the lead does more than escalate.
