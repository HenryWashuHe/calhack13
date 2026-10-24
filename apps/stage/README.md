# GENESIS stage

A judge-facing demo screen: one static HTML page, no build step. It draws Org A
and Org B as living charts, the capability that travels between them, the
ASI:One conversation and the kernel audit strip.

Two modes, labeled in the top-left badge:

- **Replay of the local fixture run** (default): the twelve steps of
  `scripts/dev-scenario.mjs` in the order the kernel recorded them. Every
  provider binding in that run is a development stub and is labeled `DEV STUB`.
  Use Play, Next step, the scene dots, or the arrow keys.
- **Live kernel**: open "Connect to a running kernel", give both organizations'
  API URLs and **agent** tokens, and the page draws exactly what the kernel
  reports every two seconds (`GET /organization`, `GET /events/log`). Nothing
  here can commit, provision or publish. The organization servers allow any
  origin, so the page can be served from anywhere on the presenter's machine.

```sh
cargo run -p genesis-server -- dev            # kernel stack with stubs
python3 -m http.server 7300 --directory apps/stage
# open http://127.0.0.1:7300/ and press "Go live" (defaults point at the dev stack)
```

Honesty: the replay is a replay and says so; live mode shows the kernel's own
labels. Neither is live sponsor evidence. The last replay scene names the
method-lineage beat as a proposal, not something that ran.
