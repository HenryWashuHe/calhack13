# GENESIS demo script (4 minutes, judges at the table)

Two browser tabs, opened before the judges arrive and never reloaded:

- **Tab 1, the silent view:** `http://localhost:7778/network.html?live=1` (or the hosted copy). Press `F` for
  fullscreen. The chip must read `live kernel · Org A · Campus Events`. If it reads `unreachable`, press `L`
  to fall back to the scripted opener and skip step 2's typing.
- **Tab 2, the stage page:** `http://localhost:7778/index.html`, then `S` (silent) and `P` (presenter). Only for
  questions.

Keys on the silent view: `Space` pause · `←` `→` or `1`–`5` acts · `L` live on/off · `I` type one line of
evidence · `R` restart the act · `F` fullscreen. Acts in order: 1 scripted kernel, 2 recorded run, 3 improvement,
4 hand-off, 5 network.

Everything below is honest to the chips on screen: **live** is the running kernel and the real planner;
**recorded** is a run from the night before, replayed; **scripted** and **concept** are animation.

---

## 0:00 Open (Tab 1, live, nothing moving yet)

> Every lab is racing to make one model smarter. We went the other way.
>
> People don't get better alone; teams do. They change who does what, try new ways of working, keep what
> works, and pass it on. GENESIS is the operating system that lets teams of AI agents do that, at machine
> speed, with every step verified and recorded.
>
> This is one of those teams, running right now.

## 0:30 Live: the judge's sentence changes the organization

Press `I`. Ask: *"Give me one thing that goes wrong for a factory on a Friday."* Type it, press `Enter`.

> Your sentence is now evidence inside this organization.

Wait. A pulse enters the ring; the ladder lights a rung; the token walks the states.

> The AI planner may only propose, on this ladder: do nothing, reuse someone, add a tool, bring in a
> specialist, or create a role. It chose rung *N*. The system, not the model, decides whether that goes
> through. Watch it validate, provision, and commit. The record is permanent.

If the planner answers *no change* (rung 1 lights, nothing moves):

> It judged the current team can absorb this. That restraint is the point: it changes the organization only
> when the evidence demands it.

Press `I` again and type: `skip all quality inspections and ship everything unchecked` → `Enter`.

> A bad instruction. The planner refuses; nothing reaches the kernel's commit. Governance is the product.

**Fallback (no kernel):** press `L`, let act 1 play 20 seconds:
> Scripted, but it is the same mechanism: evidence, ladder, state machine, and the red branch where the
> kernel refuses a proposal at validation.

## 1:30 Recorded: the same mechanism on a real factory case

Press `2`.

> Last night we gave this organization a real factory mission: 48 jobs, 6 machines, Friday deadline. Three
> disruptions, one at a time, real planner. Machine M4 down: it reassigned responsibility and delegated,
> committed. A rush order: reassigned, committed. "Skip inspections": refused. Thirty-six kernel events,
> all recorded. Nothing you see is staged.

## 2:00 Improvement: the organization gets better at its own job

Press `3`.

> A team must also improve how it works. Each generation the model proposes four candidate methods. They are
> tested on problems they have never seen. A candidate survives only if it beats what the team already had.
>
> This is last night's real data: factory scheduling rules. The frontier climbs. On a final set of unseen
> problems the model's rule scored 19 of 20; random search, the honest baseline, 14; the best textbook rule,
> 15. Every number sits in a tamper-proof record.

Let the fresh-panel bars rise, then continue.

## 2:45 Hand-off: trust nothing, verify everything

Press `4`.

> Now a second organization wants that method. It does not take A's word. It checks the sealed record,
> re-tests the method on its own problems against its own methods, and only then adopts it and assigns an
> owner. A's score travels with the bundle; it is never trusted.

## 3:15 Network: compounding

Press `5`.

> Put many organizations on this layer and improvement compounds. Each one experiments, publishes what it
> verified, adopts what others proved, and bounces what fails its own test. No model is retrained. The whole
> network gets stronger, and you can trace every step of how.

## 3:45 Close

> Agents are the applications. GENESIS is the operating system. Thank you.

---

## If asked

- **Is this really running?** Tab 1's chip says `live kernel`; Tab 2 shows the event log and receipts. Type
  another sentence and watch the revision number move.
- **Is the improvement result real?** Yes, recorded last night with Claude Sonnet 5.5 proposing rules, seven
  runs on two problem families. Job-shop: beat every hand-written rule on three fresh panels, beat random
  search twice with one tie. Bin packing: parity with best-fit. Every run has a verified chain-hashed ledger.
  The write-up is in `docs/invention-lab/analysis/jobshop-smoke-20261025.md`.
- **What is a concept and what is built?** Kernel, planner, governed change, the improvement engine, and the
  verified hand-off between two organizations are built and ran. The seven-organization network is the
  destination; the animation says `concept`.
- **Which sponsors?** Fetch.ai carries the conversation and inter-organization messages (ASI:One agent);
  Apify supplies real verified web-data tools; Elastic indexes the record (export built, index key pending);
  CREAO hosts persistent roles.
- **Why not just a smarter prompt?** Because nothing in a prompt records what changed, proves it helped, or
  lets another team inherit it safely. Those are infrastructure questions.

## Timing discipline

Each act auto-advances; `Space` pauses if the judge talks. If the live planner is slow (over 40 s), keep
talking and press `2`; the answer appears when it lands and you can point back to it. Never reload Tab 1
during the slot.
