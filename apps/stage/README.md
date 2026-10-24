# GENESIS stage

A judge-facing demo screen: one static HTML page, no build step. It draws Org A
and Org B as living charts, the capability that travels between them, the
ASI:One conversation and the kernel audit strip.

Two modes, labeled in the top-left badge:

- **Historical fixture replay** (default): an illustrative storyboard of the
  committed dev scenario (`scripts/dev-scenario.mjs`), which posts scripted
  changes and a fixture manifest marked "not built on Apify yet". The replay
  therefore shows no synthesis, build or scoring, and says so. Every provider
  binding in that run is a development stub and is labeled `DEV STUB`. Use
  Play, Next step, the scene dots, or the arrow keys. The venue scenario is the
  current regression workload, not the product; the last scene labels the
  invention-lab beat as a proposal.
- **Live kernel**: open "Connect to a running kernel", give both organizations'
  API URLs and **agent** tokens. The page clears every replay panel, disables
  the replay controls, and draws only what the kernels report every two
  seconds (`GET /organization`, incremental `GET /events/log?after=<cursor>`
  with a bounded visible tail). An unreachable kernel shows "Live kernel:
  unreachable" and no data; a response from an older connection is ignored.
  Inheritance is drawn only for an exact version Org B accepted that Org A also
  accepted and whose registry manifest names Org A as publisher; any other B
  installation is shown with its own status. Nothing here can commit,
  provision or publish. The organization servers allow any origin.

```sh
cargo run -p genesis-server -- dev            # kernel stack with stubs
python3 -m http.server 7300 --directory apps/stage
# open http://127.0.0.1:7300/ and press "Go live" (defaults point at the dev stack)
```

Honesty: the replay is a replay and says so; live mode shows the kernel's own
labels. Neither is live sponsor evidence. The last replay scene names the
method-lineage beat as a proposal, not something that ran.

Checks: `node --test apps/stage/stage.test.mjs` runs dependency-free VM probes of
the inline script (unreachable kernel, rejected installation, cursor paging,
stale responses). They are source checks, not a browser walkthrough.
