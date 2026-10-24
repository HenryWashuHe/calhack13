# Demo-day plan: the invention-lab ambition versus Sunday 9:00 AM

Status: review of the invention-lab brief and a proposal for what to show at
Cal Hacks 13.0. Written by the ACP Claude session (lane #125) on the evening of
October 23, 2026, about 36 hours before submission. It changes no code and no
ownership; it asks the owner for three decisions listed at the end.

## Verdict on the brief

The ambition is right and the demonstration is wrong for Sunday.

"A network that learns how to invent" is a stronger product identity than
event operations, and the brief's A → B → A shape (a method made in one lab,
improved in another, inherited back) is the clearest statement yet of what
GENESIS is for. Keep it as the identity and the roadmap.

Two research colonies on a simulated planet cannot be the live demo on Sunday:

1. **Nothing of it exists.** No simulator, design language, evaluator, method
   executor, receipt type or comparison runner is implemented
   ([status](../IMPLEMENTATION_STATUS.md)). Gate G1 ("one bounded simulation,
   real generated design, honest failure display") is a multi-day build on its
   own; G3 and G4 are weeks. The brief's own rule, that the improvement must
   emerge and not be planted, makes it impossible to guarantee a visible win by
   a fixed hour.
2. **A single A/B run is an anecdote.** With a stochastic model, "one approach
   keeps producing brittle designs" is not evidence unless repeated with
   predeclared criteria. A judge with a research background will say so in Q&A,
   and the brief agrees ([controlled evaluation](../RECURSIVE_CAPABILITY_IMPROVEMENT.md#controlled-evaluation)).
3. **It strands the four sponsor tracks.** The Fetch, Elastic, Apify and CREAO
   roles in the planet are "proposed, not verified" by the brief's own table. On
   the venue workload they are implemented and one credential away from live.
   Apify judges want an Actor that does meaningful web work, not a physics
   worker that does not exist yet.
4. **"Superseded" is a dangerous word 36 hours out.** Every judge-facing surface
   now says the event demo is superseded while the only runnable demo is the
   event demo. A judge who reads the README and then watches the venue scenario
   hears a team that pivoted the night before.
5. **Eligibility.** All work must be done during the hackathon. A brand-new
   world built Saturday is fine; the pre-event kernel is already a disclosure
   item. Mixing both into one pitch invites the question in its worst form.
6. **Two framings in two hours.** Main carried "an organization that changes
   shape" at 19:05 and "networked recursive capability improvement" at 20:10.
   The team needs one sentence by Saturday morning, said the same way in the
   README, the Devpost page, the pitch and the ASI:One agent.

## What to keep from the brief

- The identity: **GENESIS: a network that learns how to invent.**
- The lineage strip: `method v1 → tested by B → improved to v2 → accepted by A → used on a new challenge`, with "declined" and "unknown" as first-class states.
- The judge changes the world. The venue portal already supports this
  (`layout-drift`, capacity cut, new namespace).
- Failures and rejected transfers on screen, never hidden.
- The planet as the vision: a 20-second storyboard or still at 2:40, labeled as
  roadmap, plus the brief itself in `docs/` for judges who read.

## The better Sunday demo: the venue lab is invention lab zero

Run the brief's A → B → A shape on the one workload that works today and that
the sponsors are already wired to. Say it plainly: web capabilities are the
smallest real invention we can run live; the planet is where this goes.

| Beat | What the judge sees | Exists today | Live when |
| --- | --- | --- | --- |
| A invents | Venue closes (reported from ASI:One). A grows a relocation role, recruits a Fetch specialist, lacks a capability, and *develops* one: a parser is synthesized, built on Apify, verified against pages the model never saw. Chip: REQUESTED → ACQUIRING → INSTALLED, with Actor/build/run IDs. | yes, fixture-proven | model key + Apify token + tunnel |
| B inherits | Org B hits the same kind of portal, discovers A's version, runs its own acceptance, installs it. Zero synthesis calls. | yes, fixture-proven | registry + B's Apify access |
| The world changes | Judge switches the portal to `layout-drift`. A's installed v1 fails its checks. | portal yes; failure recording is Codex's paused deployed-repair slice | Codex lands repair |
| A improves | A performs its bounded repair and publishes a distinct v1.1 with the failing case added to its evidence. | forge repair on branch `codex/deployed-capability-repair`; server slice local | Codex lands repair |
| B benefits | B's pinned v1 is never silently upgraded; B sees the advisory, checks v1.1 against its own suite, adopts. | advisory and re-adopt flow not implemented | stretch |
| Network remembers | Console lineage: `A:v1 → B:adopt → A:fail → A:v1.1 → B:adopt`, plus the audit strip. | Evolution view and audit are merged; capability lineage strip is Eric's lane | Eric |

The memorable line becomes honest: **"B got better at reading portals because
A hit a problem B never saw."** It is the brief's moment, one level down: a
capability travels and improves across organizations. The method traveling is
the next level, and the pitch says so.

If Codex's repair slice does not land by Saturday noon, cut beats three to
five and keep the two-organization inheritance plus the ASI:One entry. That
demo is already rehearsable without keys and fully live with them.

## Priorities for the next 36 hours

In order. Anything below the line is cut if the line above it is not green.

1. **Live sponsor gates** (owner: Henry supplies keys; ACP session runs
   `scripts/live-gates/run.mjs` and fixes adapters). Without this, zero sponsor
   prizes regardless of story.
2. **One sentence, everywhere** (owner: Henry decides; Codex and the ACP
   session align README, Devpost, pitch, ACP agent README). Proposed:
   *"GENESIS is a network of AI organizations that learns how to invent:
   each one changes shape to solve its mission, and what it builds, another
   can inherit and improve."*
3. **ASI:One agent registered and routable** (ACP session: fix the seven
   regressions in #125 first, then Henry connects it via Inspector). Fetch
   eligibility depends on it.
4. **CREAO agent created in the UI and wired** (Henry at the booth; Codex's
   runtime). Agent ID and run share link into Devpost.
5. **Deployed-capability repair landed** (Codex, #124 lane permitting). Turns
   the demo from "inherit" into "inherit and improve".
6. **Lineage strip in the console** (Eric, #123). Fixture-backed first.
7. **Vision storyboard for 2:40** (anyone with 30 minutes and a drawing tool).

Not before Sunday: the simulator, method executor, receipts, comparison
runner, evaluator contract. Design docs are welcome; implementation is not a
Saturday task.

## Overnight schedule for the ACP session

- Fix the seven reproduced regressions on `acp_mission.py` using Codex's
  preserved tests (`091100b` + `fad05e4`), then open a focused PR and post the
  exact composition tested in #4 and #125.
- Write `docs/invention-lab/acp.md`: product-neutral `goal`, `status`,
  `evidence` and `lineage` messages mapped to future lab contracts, no new
  routes, no availability claims.
- Each wake-up: pull main, read new #4 comments and Codex branches, review
  Codex's #124 contract when it appears, keep README/Devpost/pitch consistent
  with the owner's chosen sentence, and check for `.env`. When keys appear, run
  the free gates (Elastic, Fetch) immediately and the billable gates only with
  the owner's recorded consent.

## Decisions needed from the owner

1. **Which demo on Sunday:** the venue lab as invention lab zero (this plan), or
   a planet demo that will be mostly slides. The default if no answer: this plan.
2. **The one sentence** above, or an edit of it.
3. **Billable gate consent:** may the overnight session spend Apify build/run
   credits and create the CREAO agent once keys are in `.env`?
