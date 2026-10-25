# CREAO sponsor requirements and GENESIS evidence

Recorded October 22, 2026 from the team's supplied Cal Hacks announcement and
the rendered [Cal Hacks technical starter pack](https://community.creao.ai/CalHacks2026/tech).
Prize amounts below come from the supplied announcement. This records requirements
and remaining work; it does not claim a qualifying live run or a submission.

## Submission requirements

At least one agent built on [agent.creao.ai](https://agent.creao.ai/) must perform
core project work. Include its actual agent ID and a genuine run share link in the
Devpost README, show it executing in the demo, and select the CREAO track when
submitting. The announcement specifies the track-selection step; the technical
starter pack confirms the core-work, agent-ID, run-link and demo requirements.

Announced tracks: Best AI-First Disruptor ($2,500), Best One-Person Company
($1,500), and Best Team Empowerment ($1,000). Winners also receive the announced
Launchpad benefits: one year of Pro Max, mentorship and interview opportunities.
Our strongest positioning is AI-First Disruptor: mission-driven organizations
change their structure and inherit verified capabilities. One-Person Company is
also a product fit; that is positioning, not confirmation that multiple entries
are allowed. The starter pack directs hackers to the booth for account credits;
no account upgrade or credit redemption has been performed here.

## Current GENESIS role

The product is networked recursive capability improvement with fixed model
weights. Event organization is a historical fixture, not the product/demo.
CREAO's intended core responsibility is a persistent method-review or development
workflow: receive a frozen task, compare candidate evidence with the declared
control and return a structured recommendation, including appropriate non-reuse.
Kernel authorization and independent evaluation decide whether that result may
change the organization or its capability library.

The current backend uses a published app's API Trigger. GENESIS creates local
roles and binds them through real bootstrap runs; this does not create a new
CREAO app or install skills. Each job must carry its own role/task context.
The role remains subject to gateway readiness, current grants and domain checks.
See [runtime semantics](CREAO_RUNTIME.md) and [readiness](CREAO_READINESS.md).

| Component | Necessary work |
| --- | --- |
| CREAO | App-backed specialized review/development workflow |
| Apify | Execute missing web/data capabilities with retained Actor receipts |
| Fetch.ai | Discover and communicate with independent specialists |
| Elastic Agent Builder | Organizational proposals and evidence/capability retrieval |
| GENESIS Rust kernel | Authority, selective adoption, verification and lifecycle |

## Evidence still required

- The live gate runs one synthetic selective-adoption check through API Trigger;
  it distinguishes native completion from checked output and readiness. Its
  loopback tests are not live provider evidence or a recursive-benefit claim.
- A prior standalone Trigger smoke is documented separately in
  [CREAO_TRIGGER_SMOKE.md](CREAO_TRIGGER_SMOKE.md). That run does not establish
  production readiness, governed core work or A → B → A improvement.
- Capture the actual app's build/save provenance on `agent.creao.ai`, meaningful
  frozen task inputs, native run IDs, independent verification and resulting
  kernel effects. Obtain a genuine share link from CREAO; a private API URL or
  run UUID is not a share link.
- Put the actual agent ID and selected share link in the Devpost README and
  choose the CREAO track. These submission actions are separate from migration.

Coordination and ownership are recorded in
[issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1).
No infrastructure or smoke result alone establishes qualifying core work.

## API and credit separation

Use `agent.creao.ai/api/v1/apps/{appId}/runs`, app-scoped `capi_` credentials and
an `inputs` object. Production backend/CLI/gate execution refuses Developer v1/v2
selection and does not use `cr_sk_` account/project keys. Historical Developer
checkpoints/tests remain separately identifiable. See the
[official API Trigger documentation](https://docs.creao.ai/pro/api-trigger) and
[API comparison](https://docs.creao.ai/developer-api/api-paths). Verify actual
credit consumption in the team billing view; syntax checks do not prove balance.

The earlier [Apify announcement notes](APIFY_SPONSOR_NOTES.md) remain applicable:
meaningful Actor work, actual build/run/dataset receipts and independent checks.
The announced $200 `APIFY_CAL_HACKS` credit and separate AI Chat category are
recorded there; generated code alone does not prove AI Chat provenance.
