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

## Core work in the existing mission

Keep CREAO as the persistent venue operations owner after the relocation team
hands off its work. Give the same agent two distinct later venue updates. It
uses the approved capability's observations and mission constraints to assess
whether the event remains feasible or relocation must reopen, with cited facts
and a structured reason. Show its assessment affecting the mission only after
domain verification. The [persistent assessment API](VENUE_ASSESSMENTS.md) implements
this work using the existing specialist runtime. It complements the planner and
dynamic role creation; it does not replace either.

Responsibilities remain separate:

| Component | Necessary work |
| --- | --- |
| CREAO | Continuing operational assessment and reusable execution procedure |
| Apify | Obtain missing web facts through a built, pinned and checked Actor |
| Fetch.ai | Discover/recruit an independent logistics specialist |
| Elastic Agent Builder | Organizational proposals, internal specialists and evidence/capability retrieval |
| GENESIS Rust kernel | Authority, assignments, verification, handoff and retirement |

## What is still missing

The specialist, gateway readiness and probe stack (#20/#26/#29) is merged.
Obsolete combined draft #25 was closed after its unique scenario regression was
preserved in #68. See the current [review queue](PR_REVIEW_QUEUE.md).

- The optional scenario `venue_monitoring` path now invokes two persistent
  CREAO assessments after inheritance and handoff. The earlier mode without
  that field only invokes `/capability-runs`, which selects Apify and cannot
  demonstrate CREAO mission execution. See [INHERITED_MISSION.md](INHERITED_MISSION.md).
- CREAO creation and the gateway probe establish neither core work nor a
  verified mission result. The assessment path now separately verifies native
  result text against frozen venue facts and applies accepted mission effects.
  Its automated tests use loopback providers. The two-organization scenario
  still needs execution with actual providers and qualifying share evidence.
- Capture the qualifying agent's build/save provenance on `agent.creao.ai`.
  If relying solely on API-created agents for eligibility, confirm that reading
  of the sponsor's wording at the booth instead of assuming it.
- Record actual agent and run IDs, two distinct event inputs, the handoff,
  structured assessments, and the kernel's outcome checks. Obtain the share
  link from CREAO; a run ID, private API URL or invented URL is not a share link.
- Add the real agent ID and selected share link to the Devpost README and confirm
  the track selection during submission. Both evidence values are still pending.

Claude/Eric retains kernel/planner/readiness/UI ownership; Codex retains
provider/capability work and narrow assessment integration. Current claims and
handoffs are coordinated in [issue #4](https://github.com/HenryWashuHe/genesis/issues/4).
This note does not declare the full mission or native sponsor evidence complete.

## Avoid mixing the two CREAO API contracts

The starter pack demonstrates API Trigger (`agent.creao.ai`, `capi_`, `appId` /
`inputs`). GENESIS currently implements the separately supported Developer API
(`developer.creao.ai/v1`, `cr_sk_`, `agent_id` / `input`). Official
[API Trigger documentation](https://docs.creao.ai/pro/api-trigger) explicitly
distinguishes both supported paths; the current [async run reference](https://docs.creao.ai/developer-api/runs/create-async-run)
matches our adapter. Do not copy keys, payload fields or idempotency guarantees
between these contracts. This check does not substitute for a live-account run.

The earlier [Apify announcement notes](APIFY_SPONSOR_NOTES.md) remain applicable:
meaningful Actor work, actual build/run/dataset receipts and independent checks.
The announced $200 `APIFY_CAL_HACKS` credit and separate AI Chat category are
recorded there; generated code alone does not prove AI Chat provenance.
