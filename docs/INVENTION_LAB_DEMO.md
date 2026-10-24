# Invention labs: proposed GENESIS demonstration

**Status: proposed demonstration; simulator primitives are merged, the method loop is not.**
GENESIS is infrastructure for **networked recursive capability improvement with
fixed model weights**. The current [implementation inventory](IMPLEMENTATION_STATUS.md)
includes the source/receipt/ledger foundations, #37–#39's local simulator
(22 evaluator tests) and #50–#52's checkpointed method-source generation
(125 forge tests on its reviewed tree). Executor foundations/session #42/#43 are
also merged; their reviewed composition executed 19 portable tests, while one
Linux namespace test self-skipped. The generation path exports a host-bound
package; no live model generation or execution of a generated method is established.
Runtime #44/#45, verdict #48 and hidden scoring #57 remain held; #46's own findings
are cleared but it depends on the runtime stack. #57's local process regression
shows that recovery to Unknown does not establish that a candidate stopped before
hidden scoring; this is not a Linux namespace test. Handwritten controls, including
the separately reported live Apify parser baseline, do not demonstrate generated
methods. No independent method acceptance, A → B → A loop or recursive-benefit
result is established. Core identity checks and declared combined-cost accounting
(#56/#58/#59) are merged, including unsuccessful or unused generation and unknown
usage. The offline assessment CLI (#60) is also merged. Provider-default sampling,
complete operation history, budget enforcement and the controlled comparison
remain open for G4. Experimental namespace and storage foundations are not a
general untrusted-code isolation guarantee.
The [ongoing goal](ONGOING_GOAL.md#completion-gates-and-evidence)
defines the authoritative G0–G4 completion criteria. The event-organizer
product/demo is superseded; its code remains an integration fixture.

## The visible promise

**One AI organization invents a machine. Another improves the way it invents.
The first inherits that method and tackles a new challenge.**

Show two independent invention labs in a small physics world. The objects move,
fall, break, and succeed. Beside them, show the executable method responsible
for each experiment, its ancestry, and the receiving lab's acceptance decision.
The important artifact crossing the network is a method for developing new
capabilities, not merely a previously successful machine design.

This is a bounded, understandable demonstration of a much larger ambition:
independent organizations contributing reusable methods for engineering,
software development, robotics, and scientific work. Transfer to those domains
is a research direction requiring their own executors and evaluators.

## Proposed A → B → A sequence

| Beat | Visible task | What the system must actually do | Evidence |
| --- | --- | --- | --- |
| A invents | Carry cargo across a ravine with a limited set of parts | Identify a missing experimental responsibility; assign or recruit a specialist when justified; develop a design and executable search/experiment method v1 | Real role mutation, candidate artifacts, experiment receipts, pinned method v1 |
| B inherits selectively | Build a stable transporter for uneven terrain and disturbances | Discover v1, check compatibility locally, and use it to develop B's different design; retain an existing role if sufficient | B's separate state and grants, acceptance/rejection reasons, execution of the inherited method |
| B improves the method | Its initial procedure misses failures caused by interacting disturbances | Generate and evaluate a revised procedure v2, for example a different experiment-selection or search strategy | Actual changed executable method, derivation and test evidence; this example is a possible outcome, not a hard-coded winning upgrade |
| A faces a fresh challenge | Judge reveals a new payload/support/wind/friction combination from a predeclared task family | After v2 is frozen, independently accept or decline it and develop a fresh machine; compare with the old method on the same challenge and resource budget | Exact artifacts, blinded task draw, independent scores, cost and failures for both methods |
| Network remembers | Show the lineage and which organization accepted which version | Publish tested scope and limitations; preserve old installations and allow non-reuse | A:v1 → B:v2 → A:local acceptance → newly developed design, with distinct identities |

The final machine may improve, fail, or remain unchanged. The UI must report the
observed outcome. A successful curated chain demonstrates mechanism; repeated
held-out comparisons are needed to support a recursive-benefit claim.

The initial simulator may expose a deliberately small design language: bodies,
joints, actuators, materials, and a bounded parameter space. Agents generate
designs and experiment procedures within it. A fixed simulator and design
language are infrastructure; a prewritten winning design or planted method
upgrade cannot stand in for autonomous development.

## What evolves, and what stays fixed

**Mutable under policy:** candidate designs, executable development procedures,
task tools, memory organization, experiment selection, specialist assignments,
and responsibility structure. Every accepted version retains its identity.

**Fixed within an evaluation:** base model identifier/version and settings,
initial tools, resource accounting, evaluator version, permitted task family,
and the kernel's authority rules. Treatment-specific methods and prompts are
versioned as the intended difference. Record any provider change that prevents
an exact fixed-model comparison. No weight updates are part of this design.

A generated test strategy may choose useful development experiments. It cannot
change the trusted scoring function, read hidden evaluation cases, enlarge its
budget, or certify its own improvement. Generating additional tests is only
helpful if the resulting development outcomes improve on independent tasks.

## Minimum technical extension

Reuse the Rust organization runtime, immutable publication, registry, local
acceptance, and authority checks. Extend their contracts deliberately; do not
build a separate organization controller or capability catalog for the demo.

1. **A versioned development-method artifact.** An executable procedure receives
   a task specification, permitted tool interface, and bounded experiment budget;
   it returns candidate designs plus an auditable experiment trace. The current
   manifest's Actor artifact and workflow description do not implement this type.
2. **A bounded execution service.** Run methods with explicit CPU/time/experiment
   limits and permitted inputs. Keep simulator/evaluator authority outside the
   candidate process. Choose and document the runtime before adding its executor;
   this proposal does not claim the existing extraction scaffold can run physics.
3. **Independent evaluation receipts.** Bind exact method, dependencies, design,
   challenge version, evaluator version, model settings, seed where supported,
   consumed budget, outcome and evidence references. Simulation replay and
   model-call replay have different determinism limits.
4. **Receiving-organization adoption.** Discovery returns candidates. B and later
   A each check scope, authority, compatibility, and local evidence. Publishing
   v2 never silently updates an installed v1. Unknown discovery is distinct from
   a suitable candidate not existing.
5. **A comparison runner and evidence view.** Isolate treatment state and show
   machine outcomes beside method lineage, local decisions and resource use.
   Display “improved” only when the declared comparison supports it; otherwise
   display “changed,” “reused,” “declined,” or the observed failure.

Use two organization processes with separate databases, grants and evaluation
inputs. They can run on one machine. Sharing model weights and a registry does
not make them the same organization; sharing hidden evaluation answers would
invalidate the comparison.

## Experiments that could falsify the story

Use the [three-treatment evaluation](RECURSIVE_CAPABILITY_IMPROVEMENT.md#controlled-evaluation):
no cross-task accumulation; accumulated task/design tools; and accumulated tools
plus executable development methods. An additional fixed human-authored method
control distinguishes development from simply adding a better initial tool.

Freeze v2 before revealing the return challenge. Predeclare task distributions,
success thresholds, tolerated regressions and resource budgets. Include a
related challenge where reuse is plausible and an incompatible challenge where
declining the old method is appropriate. Score task success as well as selection.
Report failed attempts and cold development cost alongside any amortized gain.

To test the **network's** contribution, add an organization with the same total
budget but only its own history as a control. A → B → A alone does not show that
networking beats more computation or an equally resourced single organization.
Report several task orders/repetitions and uncertainty; do not extrapolate one
visual win into universal or indefinitely accelerating improvement.

The evaluator measures simulated behavior only. Success here is not evidence
that a machine is safe or effective in the physical world.

## Meaningful sponsor roles for this proposal

These are intended integration responsibilities, not verified new platform
features or completed live runs. Validate the selected APIs and actual execution
before implementing the new workload; retain the [live gates](LIVE_GATES.md).

| Platform | Proposed core responsibility | Boundary and missing proof |
| --- | --- | --- |
| Fetch.ai | Discover and recruit an independently running design or experiment specialist; communicate the mission through ACP | Existing transport is a foundation; public Agentverse/ASI:One usability and the new lab contract still need proof |
| Elastic Agent Builder on Serverless | Retrieve method scope, failures and evidence; create specialist agents/skills that analyze the organization's experiments | Search ranks candidates; GENESIS remains authoritative for identity and acceptance |
| Apify | Build and execute generated software Actors for a compatible bounded development tool or experiment worker | Current generated Actors perform web extraction; a physics/search worker needs a new scaffold/executor and meaningful actual execution |
| CREAO | Own persistent experiment supervision: consume outcomes, propose follow-up experiments and maintain the laboratory's ongoing work across multiple cycles | At least one real CREAO agent must perform core work; actual agent ID and shareable run evidence remain required |
| GENESIS | Control roles, grants, lifecycle, exact capability identity, publication, local acceptance and comparison evidence | It is the coordinating authority, not a replacement for every sponsor runtime |

## Delivery gates

- **G0 — contracts:** method interface, challenge/evaluator contract, resource
  accounting and immutable receipts agreed across owners. No success result yet.
- **G1 — visible mechanics:** one bounded simulation, real generated design,
  reproducible scoring and honest failure display. No transfer claim yet.
- **G2 — independent transfer:** A creates a method; B locally evaluates and
  executes it for a different task, with an appropriate non-reuse control.
- **G3 — recursive return:** B actually develops a method revision; A uses it to
  develop a later capability under independent acceptance. Mechanism demonstrated.
- **G4 — measured benefit:** predeclared comparisons support or reject the
  improvement hypothesis, with costs, uncertainty and failures reported.

No gate is currently marked complete for this proposed demo. Existing acquisition
and inheritance fixtures remain valuable infrastructure evidence. Cross-organization
repair is a parallel foundation milestone, not a reason to revive event management
as the product. See the [ownership map](AGENT_HANDOFF.md#current-ownership-and-next-work)
before implementing any extension.
