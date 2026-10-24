# GENESIS: the 3-minute pitch

Cal Hacks 13.0. Three minutes to pitch, two for questions. One presenter talks,
one drives the console and the ASI:One chat. Every beat below names what is
on screen and whether it is live, a fixture, or recorded. Say the label out
loud. The honesty table in [DEMO_RUNBOOK.md](DEMO_RUNBOOK.md) is the source
of truth; if a beat has not passed its live gate by demo time, read its
fixture clause instead of the live one.

**Recorded-segment label (say it, and show it as a caption):**
`Recorded Oct 25 02:10 PT, one unedited run, same build as the live stack.`

## The script

**0:00 to 0:20. Hook. Screen: console, Org A tree with three roles.**

Every AI agent product ships a fixed org chart. Real organizations survive
because they change shape. They hire, build tools, hand work off, dissolve
teams, and keep what they learned. GENESIS is the operating system that lets
an AI organization do that safely. This is Org A. It runs a campus event for
120 people. Three roles, a budget, a constitution. Nothing in the code knows
what happens next.

**0:20 to 0:40. The disruption. Screen: ASI:One chat on the left, evidence log on the right.**

We talk to the organization through ASI:One. "The main hall is closed for
November 14." That one sentence becomes an observation in the evidence log.
[LIVE if the ACP gateway is registered; otherwise: "we post the same
observation from the operator script".] The evidence cites the venue portal.
No human writes a plan.

**0:40 to 1:15. The organization changes shape. Screen: Changes panel, then the tree.**

The planner reads the evidence and the constitution and proposes the smallest
valid change. The kernel validates it. Watch the tree: a Relocation lead
appears. It did not exist thirty seconds ago. It owns one responsibility,
"relocate the event", with a budget and scopes the constitution allows. It
recruits an independent logistics specialist over Fetch.ai, under a signed
contract. Then it says: I cannot read this venue portal. That is a
capability gap. The chip says REQUESTED. [Planner LIVE with a model key;
Elastic specialist LIVE with Elastic keys; otherwise labeled DEV STUB on
screen, and we say so.]

**1:15 to 1:45. The organization grows an ability. Screen: gap chip ACQUIRING to INSTALLED, Apify IDs.**

GENESIS writes an Apify Actor for that portal, builds it, and runs it against
pages the model never saw. The oracle says 3 of 3. The chip moves to
INSTALLED, pinned to an exact build. Here is the Actor ID, the build number,
the run ID. [LIVE with Apify and model keys and the tunnel up; cold build
time decides live versus recorded, see the label.] The lead runs it and
reserves Hall B in the ledger.

**1:45 to 2:10. A second disruption, a different answer. Screen: Venue monitor role, Attention panel.**

Relocation is done, so the lead hands monitoring to a persistent CREAO agent
and retires. Now the judge cuts the new hall's capacity. No new role this
time. The existing monitor reassesses, cites the facts, and opens one
replacement requirement. Two disruptions, two different organizational
responses. That is reasoning, not a script. [CREAO LIVE with the account key
and gateway callback; fixture otherwise.]

**2:10 to 2:40. Another organization inherits. Screen: switch to Org B tab.**

Org B is a different company with its own mission, database and keys. It
hits the same kind of portal. It searches the registry, finds A's verified
capability, runs A's exact pinned build against its own acceptance suite, and
installs it. Model calls by B: zero. Synthesis calls by B: zero. What one
organization learned became an ability another one owns. [Registry and
adoption LIVE in-process; B's Apify run of A's Actor needs B's own token.]

**2:40 to 3:00. Proof and close. Screen: Evolution view, then the Audit strip.**

Replay shows every generation of this organization and why it happened. The
audit strip checks every revision ever stored against 27 kernel invariants.
A late result from the retired role's epoch arrives and is rejected. Every
state this organization ever held is still well formed. GENESIS: the
organization a mission needs, and the abilities it leaves behind.

## Timing notes

- Rehearse to 2:45. Q&A eats overrun.
- If the cold Actor build runs long, keep it building in the background and
  play the recorded segment with its caption. Never call a prebuilt Actor
  fresh.
- If `needs_reconciliation` shows, say it: "GENESIS refused to guess whether
  the provider acted." Then show the Attention panel.

## Q&A prep: the ten hardest questions

**1. What did you build during the hackathon?**
The repository started two days before doors opened, as research, the
technical specification and the first kernel code. We disclose that on the
Devpost page. During the event we built the ASI:One conversation path, ran the
live sponsor gates, fixed and landed the provenance console, wrote the pitch,
and did the end-to-end runs. The commit log in the repository shows exactly
when each piece landed.

**2. What is live right now and what is a fixture?**
The kernel, saga, fencing, retirement, registry, adoption and console are
always live. The planner is live when a model key is set. Apify synthesis,
build and verification are live with a token and a reachable portal. Elastic,
Fetch and CREAO are live when their keys are set and their gate in
`scripts/live-gates` has passed. Anything else is labeled DEV STUB on screen.
We never show a fixture as live.

**3. Why not one agent with tools?**
One agent has no durable ownership. When it dies, who is accountable for the
event? GENESIS gives every requirement exactly one accountable owner, with
scopes, a budget and an epoch. A late result from a retired owner is rejected
with `stale_epoch`. Try that with a prompt.

**4. How is this different from CrewAI, AutoGen, or AG2 CaptainAgent?**
Those build a team for a task and throw it away. Roles, agents, tools and
responsibilities are not separate durable objects, and nothing survives the
run. GENESIS has a constitution, an append-only history, provisioning sagas,
verification gates, and inheritance across independent organizations. The
closest research systems, CaptainAgent, MegaAgent, EvoAgent, describe
evolution offline and inside one process.

**5. How does Org B trust Org A's capability?**
It does not trust A. It trusts its own checks. The registry carries a pinned
build, a verification report and required permissions. B runs the exact pin
against B's own held-out suite before installing. A's secrets, conversations
and mission data never leave A.

**6. What happens when the model is wrong?**
The planner cannot commit anything. The kernel rejects invalid proposals with
exact error codes, feeds them back, and allows three attempts. Scopes outside
the constitution, budget overruns, unknown IDs and missing owners are all
rejected. If no valid proposal exists, the planner escalates to a human and
the Attention panel shows why.

**7. Who pays for this? What is the business?**
The buyer is a small company that wants to run on agents without hiring an
ops team: event agencies, property managers, clinics. They pay per active
role per month, plus a share of every capability inherited through the
registry. The registry is the moat: verified capabilities with provenance,
not prompts.

**8. Why Rust?**
The kernel is a state machine with authority, budgets and fencing. We wanted
the compiler to enforce the type boundaries and the runtime to be cheap to
host per organization. Axum and Tokio handle the saga and SSE. The result
passes 600 tests and a full-history invariant audit.

**9. What stops the organization from spawning roles forever?**
The constitution caps active roles and contracts and total budget. Every new
role needs a distinct continuing responsibility and a stated justification.
Reuse and retirement are preferred in the planner's decision order.

**10. What is the moonshot?**
A network of organizations that change shape as missions change, and that
inherit each other's verified abilities with provenance. The event agency is
day one. The platform is the kernel plus the registry.

## One-liners to keep in your pocket

- "The role you just saw was not in the code."
- "Zero model calls for Org B."
- "Every state this organization ever held is still well formed."
- "GENESIS refused to guess."
