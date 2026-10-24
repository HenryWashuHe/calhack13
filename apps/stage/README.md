# GENESIS stage

A judge-facing demo screen: one static HTML page, no build step. It draws Org A
and Org B as living charts, the capability that travels between them, the
ASI:One conversation and the kernel audit strip.

The page starts at **No source selected**, with no replay or live evidence shown.
Choose one of two modes, labeled in the top-left badge:

- **Historical fixture replay** (explicit opt-in): an illustrative storyboard of the
  committed dev scenario (`scripts/dev-scenario.mjs`), which posts scripted
  changes and a fixture manifest marked "not built on Apify yet". The replay
  therefore shows no synthesis, build or scoring, and says so. Every provider
  binding in that run is a development stub and is labeled `DEV STUB`. Use
  Play fixture replay to opt in, then Next step, the scene dots, or the arrow
  keys. The venue scenario is a historical regression workload, not the product;
  the last scene labels the
  invention-lab beat as a proposal.
  While replaying, a judge can **throw a disruption** (buttons under the
  header, or keys 1 2 3): the caterer cancels, the shuttle falls through, or
  a hostile instruction asks for a Root role. The first two replay the
  transcript of a dev-stack run on 2026-10-24 (planner
  `anthropic/claude-sonnet-5-5`, provider dev stubs, fixture evidence refs);
  the third is the fixture's hostile-evidence case. Their rows carry a `D`
  sequence and the word "scripted", the audit line says those revisions are
  not audited, nothing is called, and Play fixture replay clears them.
  The story opens on the thesis ("an AI organization that changes shape and
  keeps what it learns") and runs along a named chapter rail (Thesis,
  Evidence, Decision, New role, Capability, Handoff, Inheritance, Audit).
  Every scene carries proof chips: green for the path that is the real kernel
  in that scene, dashed amber for the fixture stand-in or the proposal, so a
  judge never has to guess what is being claimed. Roles that changed since
  the previous scene are spotlighted. A planner verdict, whether a thrown
  disruption's transcript or a live `planner_decision` event, is shown as one
  decision card naming its source, the model and the timing the record
  supports (the transcript's planner-call duration; live, the kernel's own
  observation-to-decision interval).
- **Presenter mode** (Present button, key P, Esc to leave): the same page at
  projector scale with the chrome hidden and a caption naming the scene, the
  source and the keys. Fullscreen is requested but optional.
- **Live kernel**: open "Connect to a running kernel", give both organizations'
  API URLs and **agent** tokens. The page clears every replay panel, disables
  the replay controls, and draws only what the kernels report every two
  seconds (`GET /organization`, incremental `GET /events/log?after=<cursor>`
  with a bounded visible tail). An unreachable kernel shows "Live kernel:
  unreachable" and no data; a response from an older connection is ignored.
  Inheritance requires distinct organization IDs, matching accepted capability
  IDs, versions and nonempty source digests, and a returned manifest matching
  that exact identity with Org A as publisher. Any other B installation is shown
  with its own status. Nothing here can commit, provision or publish. The organization servers allow any origin.

```sh
cargo run -p genesis-server -- dev            # kernel stack with stubs
python3 -m http.server 7300 --directory apps/stage
# open http://127.0.0.1:7300/ and press "Go live" (defaults point at the dev stack)
```

Honesty: the replay is a replay and says so; live mode shows the kernel's own
labels. Neither is live sponsor evidence. The last replay scene names the
method-lineage beat as a proposal, not something that ran.

Checks: `node --test apps/stage/*.test.mjs` runs dependency-free VM probes of
the inline script, including opt-in replay, manifest identity, cursor paging and
stale responses. These source checks are not a browser walkthrough.
