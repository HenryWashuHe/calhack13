# GENESIS demo script (4 minutes, judges at the table)

Prepare two tabs before projection:

- **Tab 1, the visual view:** `http://localhost:7778/network.html?live=1&a=<encoded API URL>&mission=<mission ID>`.
  Enter the agent token in the prompt before sharing the screen; it stays in page memory.
  Never put credentials in a URL. Old `tok` parameters are removed and ignored; this
  cannot erase a URL already stored by browser history or a proxy. The token can
  propose organizational changes; this page only uses it for reads and observation posts.
  The chip says `live kernel` only after the initial event backlog is consumed. It
  authenticates connectivity to that kernel, not the planner model or provider identities.
- **Tab 2, the stage page:** `http://localhost:7778/index.html`, then `S` (silent)
  and `P` (presenter). Only for questions. Its fixture replay remains a scripted
  historical integration case; it is not the factory recording.

Keys on Tab 1: `Space` pauses motion · `L` live on/off · `I` reports evidence ·
`F` fullscreen. **Press `L` to leave live mode before using** `1`–`5`, arrows,
`R`, or the act dots. Acts: scripted kernel, recorded factory run, recorded
improvement, concept hand-off, concept network. `Space` freezes the drawing while
polling continues; newly received events wait for resume. This does not pause the kernel.

With several missions, `I` requires a current mission ID; the page never silently
chooses the first mission. A report clears only after an observation receipt.
A refusal keeps the input. A timeout, missing receipt, or server failure can mean
that the observation was accepted: check the event log before resubmitting. No
automatic POST retry is performed. If that outcome is unknown, posting stays
blocked in this page, including after reconnecting.

## 0:00 Open — one organization

> GENESIS explores networked recursive capability improvement with fixed model
> weights. The agents can change who does the work and which executable methods
> they use. They retain what happened so another organization can evaluate a
> capability for itself.
>
> This first view is connected to one running kernel. The later scenes separate
> recorded evidence from the larger vision.

If the connection is unavailable, press `L` and introduce the opener as scripted.
Do not describe a scripted animation as a running organization.

## 0:30 Live — evidence, proposal, governed change

Press `I`, confirm the factory mission, type one disruption, and press `Enter`.
Wait for the observation receipt before saying it was recorded.

> The report is now evidence. The planner may propose no change, reuse a role,
> attach a capability, recruit a specialist, or create a role. The kernel checks
> the proposed change. We are watching its recorded states, not assuming the
> planner's proposal succeeded.

Describe the outcome actually observed. A `no_change` decision is legitimate;
a failed change is not a successful refusal or a successful reorganization. A
live model response is not guaranteed by the page. For a hostile instruction,
show what the planner actually decides instead of promising a refusal.

If the response takes too long, **press `L`, then `2`**. Returning with `L`
starts a new live session at the current tail; it does not replay an answer that
arrived while disconnected. Tab 2's event log can be used to inspect it.

## 1:30 Recorded — factory planning decisions

Press `2` after leaving live mode.

> This is an author-retained extract from a dev-stack factory mission. The
> reported planner model was Claude Sonnet 5.5; the providers were development
> stubs. A machine failure led to a responsibility change and delegation; a rush
> order led to another responsibility change. Both changes committed. An
> instruction to skip inspections received `no_change` from the planner.

The original sequence spans events 108–143 and revisions 23–31. The animation
selects events and compresses time; it does not retain every intermediate
revision and is not an independent replay of execution. The evidence remains in
`recordings/factory-20261025.json`. No jobs were shown being physically scheduled
or manufactured. The first scripted act is the distinct example of kernel rejection.

## 2:00 Recorded — candidate development

Press `3`.

> Here are retained exploratory job-shop results. Each generation compares
> candidates on its selection panel, then the final chart compares the selected
> rule with controls on a fresh panel. These are separate comparisons.

Use the values on the currently displayed seed. Do not describe the line across
changing selection panels as a matched-panel improvement trajectory. The seed
rotates, so do not recite a single seed's score as the result of every run.
The source write-up is `docs/invention-lab/analysis/jobshop-smoke-20261025.md`.

> These observations motivate the controlled comparison. They do not yet show
> that accumulating development workflows improves future capability development.

A chain-hashed ledger can reveal changed or reordered retained records when its
reference hashes are trusted. It is not tamper-proof or independent execution proof.

## 2:45 Concept — selective transfer

Press `4`.

> This scene illustrates the intended transfer path: B checks a package, tests it
> against its own needs and controls, then chooses whether to adopt it. The counts
> here are illustrative. A score from A is not a substitute for B's decision.

The page does not execute this scene. The separate receiving/adoption rehearsals
have their own retained evidence and limits; this animation does not establish
an autonomous A-to-B-to-A development-method loop.

## 3:15 Concept — the moonshot

Press `5`.

> The destination is a network of organizations that can share and compose
> verified methods, including methods for developing better capabilities. Useful
> results may compound without changing model weights. Transfer remains selective:
> a method may fail elsewhere, and declining it can be the right result.

The seven-organization network is a concept animation. Universal infrastructure
does not imply universal improvement, positive benefit after every task, or a
completed recursive-benefit comparison.

## If asked about sponsors or implementation

Describe each integration from its retained evidence and the current repository
status. A connectivity chip, a development stub, an export implementation, or a
historical fixture does not establish authenticated cloud execution or sponsor
qualification. Use the sponsor notes and `docs/IMPLEMENTATION_STATUS.md` for
specific remaining evidence instead of presenting every integration as live.
