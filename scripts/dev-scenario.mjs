#!/usr/bin/env node
// DEV-ONLY scenario driver for the GENESIS kernel.
//
// It posts the same HTTP calls that the real actors will make once their
// workstreams land: the Elastic mission agent (observations, requirements,
// change proposals), the Fetch bridge (contract offer/accept), provider
// adapters (job results) and the Apify verifier (a verified manifest).
// Nothing here is a live sponsor integration. Every provider binding it
// causes is a DEV STUB, and the published manifest says so in its
// verification report.
//
// Usage: start `cargo run -p genesis-server -- dev`, then
//   node scripts/dev-scenario.mjs [--delay 1200] [--org-a URL] [--org-b URL]

const args = Object.fromEntries(
  process.argv
    .slice(2)
    .join(" ")
    .split("--")
    .filter(Boolean)
    .map((a) => a.trim().split(/\s+/))
    .map(([k, v]) => [k, v ?? "true"]),
);
const DELAY = Number(args.delay ?? 1200);
const A = { url: args["org-a"] ?? "http://127.0.0.1:7101", id: "org_a" };
const B = { url: args["org-b"] ?? "http://127.0.0.1:7102", id: "org_b" };
for (const o of [A, B]) {
  o.operator = `${o.id}-operator`;
  o.agent = `${o.id}-agent`;
  o.adapter = `${o.id}-adapter`;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let step = 0;
function say(msg) {
  step += 1;
  console.log(`\n[${String(step).padStart(2, "0")}] ${msg}`);
}

async function call(org, token, method, path, body) {
  const res = await fetch(`${org.url}/api/v1${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) {
    const err = new Error(`${method} ${path} -> ${res.status} ${data?.error}: ${data?.message}`);
    err.data = data;
    throw err;
  }
  return data;
}
const post = (org, token, path, body) => call(org, token, "POST", path, body);
const get = (org, path) => call(org, org.operator, "GET", path);

async function waitChange(org, id) {
  for (let i = 0; i < 200; i++) {
    const c = await get(org, `/org-changes/${id}`);
    if (["COMMITTED", "FAILED", "REJECTED"].includes(c.status)) return c;
    await sleep(150);
  }
  throw new Error(`change ${id} did not finish`);
}

async function propose(org, clientRequestId, proposal) {
  const rec = await post(org, org.agent, "/org-changes", { client_request_id: clientRequestId, proposal });
  console.log(`     change ${rec.change_id} ${rec.status}`);
  const done = await waitChange(org, rec.change_id);
  console.log(`     -> ${done.status}${done.committed_revision ? ` at r${done.committed_revision}` : ""}`);
  if (done.status !== "COMMITTED") throw new Error(JSON.stringify(done.errors, null, 2));
  return done;
}

const rev = async (org) => (await get(org, "/organization")).revision;
const roleByName = (state, name) => Object.values(state.roles).find((r) => r.name === name);

async function main() {
  await get(A, "/health").catch(() => {
    console.error(`Cannot reach ${A.url}. Start the stack: cargo run -p genesis-server -- dev`);
    process.exit(1);
  });
  const existing = await get(A, "/organization");
  if (Object.keys(existing.missions ?? {}).length > 0) {
    console.error("Org A already has a mission. Delete data/dev and restart the dev stack for a clean run.");
    process.exit(1);
  }

  say("Org A starts a mission with three roles");
  const m = await post(A, A.operator, "/missions", {
    goal: "Run the campus hack night for 120 attendees on Nov 14",
    constraints: ["Budget $5,000", "Venue within 2 miles of campus", "Doors open 6 pm"],
    success_checks: ["Venue for 120 reserved in the ledger", "Attendees notified 48 h before"],
    roles: [
      { key: "coordinator", name: "Mission coordinator", charter: "Owns the event plan and budget",
        authority: { scopes: ["mission.coordinate"], budget_cents: 150000 } },
      { key: "program", name: "Program lead", charter: "Owns the talk and workshop schedule",
        authority: { scopes: ["program.schedule"], budget_cents: 50000 }, reports_to: "coordinator" },
      { key: "info", name: "Attendee information", charter: "Keeps attendees informed",
        authority: { scopes: ["attendees.inform"], budget_cents: 10000 }, reports_to: "coordinator" },
    ],
    responsibilities: [
      { purpose: "Coordinate the event plan", owner: "coordinator", trigger: "mission events", termination: "event ends" },
      { purpose: "Maintain the program schedule", owner: "program", trigger: "schedule changes", termination: "event ends" },
      { purpose: "Notify attendees of changes", owner: "info", trigger: "plan changes", termination: "event ends" },
    ],
  });
  const mission = m.mission_id;
  await sleep(DELAY);

  say("Venue portal reports the main hall closed (observation)");
  const obs = await post(A, A.agent, `/missions/${mission}/observations`, {
    source: "venue-portal",
    summary: "Main hall closed for maintenance on Nov 14",
    evidence_ref: "fixture://venue-portal/closure-2026-11-14",
  });
  await sleep(DELAY);

  say("Mission agent derives an unmet requirement");
  const req = await post(A, A.agent, `/missions/${mission}/requirements`, {
    summary: "Relocate the event and coordinate attendee transport",
    evidence: [obs.observation_id],
    acceptance: "A venue for 120 within 2 miles is reserved in the ledger",
  });
  await sleep(DELAY);

  let state = await get(A, "/organization");
  const coordinator = roleByName(state, "Mission coordinator").role_id;

  say("Mission agent proposes a relocation unit (new role + external specialist)");
  const relocation = await propose(A, "dev-relocation", {
    expected_org_revision: state.revision,
    trigger_evidence_ids: [obs.observation_id],
    rationale: "Closure creates continuing relocation ownership that no existing role holds",
    budget_limit_cents: 200000,
    operations: [
      { op: "CreateRole", key: "relocation", name: "Relocation lead",
        charter: "Find and secure a replacement venue for 120 attendees",
        authority: { scopes: ["ledger.reserve_venue", "contract.logistics", "capability.venue_lookup"], budget_cents: 180000 },
        reports_to: { kind: "existing", role_id: coordinator },
        provider: "elastic", justification: "independent_ownership" },
      { op: "AssignResponsibility",
        target: { kind: "from_requirement", key: "relocate", requirement_id: req.requirement_id,
                  purpose: "Relocate the event", trigger: "venue availability changes", termination: "event held" },
        owner: { kind: "new", key: "relocation" } },
      { op: "RecruitSpecialist", key: "logistics", specialist_address: "agent1qlogistics7shuttle",
        protocol: "genesis-contract/1", scope: "Shuttle plan for 120 attendees",
        supervising_role: { kind: "new", key: "relocation" } },
      { op: "DelegateTask", responsibility: { kind: "new", key: "relocate" },
        executor: { kind: "contractor", contract: { kind: "new", key: "logistics" } },
        task: "Plan shuttle transport to the new venue" },
    ],
  });
  const contract = relocation.allocation.contracts.logistics;
  const relocateRsp = relocation.allocation.responsibilities.relocate;
  await sleep(DELAY);

  say("Fetch bridge reports the specialist's offer and acceptance");
  for (const s of ["OFFERED", "ACCEPTED"]) {
    await post(A, A.adapter, `/contracts/${contract}/state`, { state: s, sender_address: "agent1qlogistics7shuttle" });
    await sleep(DELAY / 2);
  }

  say("Verifier publishes a venue-availability capability (dev fixture manifest)");
  const digest = "9f2c" + "0".repeat(60);
  await post(A, A.operator, "/capabilities/publish", {
    capability_id: "cap_venue_availability",
    version: "1.0.0",
    publisher: "org_a",
    source_digest: digest,
    intent: "Read venue availability, capacity and price from a venue portal",
    applicability_conditions: ["Portal lists venues with capacity, date and price"],
    known_limitations: ["DEV FIXTURE: not built on Apify yet"],
    input_schema: { type: "object", properties: { date: { type: "string" }, min_capacity: { type: "integer" } } },
    output_schema: { type: "array" },
    required_permissions: ["capability.venue_lookup"],
    artifact: { actor_id: "devstub-actor-venue", build_id: "devstub-build", build_number: "0.0.0-dev" },
    responsibility_template: "Monitor venue availability",
    local_configuration_schema: null,
    acceptance_suite_ref: "suite://venue/v1",
    verification: {
      passed: true,
      cases: [
        { name: "layout-a", kind: "success", passed: true, detail: "dev fixture" },
        { name: "layout-b", kind: "success", passed: true, detail: "dev fixture" },
        { name: "missing-capacity", kind: "unsupported", passed: true, detail: "dev fixture" },
      ],
      tested_environment: "dev-scenario fixture (not a live Apify build)",
      limitations: ["dev fixture"],
      oracle_ref: "oracle://venue/v1",
      verified_at: new Date().toISOString(),
    },
    provenance: ["org_a dev scenario"],
    state: "APPROVED",
    published_at: null,
  });
  await sleep(DELAY);

  say("Org A installs, accepts and attaches the capability to the relocation lead");
  const instA = await post(A, A.operator, "/capability-installations", { capability_id: "cap_venue_availability", version: "1.0.0" });
  await post(A, A.operator, `/capability-installations/${instA.installation_id}/acceptance`, { passed: true, report_ref: "local://org_a/acceptance/1" });
  state = await get(A, "/organization");
  const relocationRole = roleByName(state, "Relocation lead").role_id;
  await propose(A, "dev-attach-a", {
    expected_org_revision: state.revision,
    trigger_evidence_ids: [obs.observation_id],
    rationale: "Relocation needs to read the unfamiliar venue portal",
    operations: [{ op: "AttachCapability", role: { kind: "existing", role_id: relocationRole },
                   capability: { capability_id: "cap_venue_availability", version: "1.0.0" } }],
  });
  await sleep(DELAY);

  say("Relocation lead runs the capability and reserves Hall B");
  await post(A, A.operator, "/capability-runs", {
    client_request_id: "dev-run-a", installation_id: instA.installation_id, role_id: relocationRole,
    responsibility_id: relocateRsp, input: { date: "2026-11-14", min_capacity: 120 },
  });
  const job = await post(A, A.operator, "/jobs", {
    client_request_id: "dev-reserve", role_id: relocationRole, responsibility_id: relocateRsp,
    task: "Reserve Hall B for Nov 14", required_scope: "ledger.reserve_venue",
  });
  await sleep(DELAY / 2);
  await post(A, A.adapter, `/jobs/${job.job_id}/results`, {
    attempt_id: job.attempt_id, epoch: job.epoch, outcome: "succeeded", provider_status: "DEVSTUB_DONE",
    output: { venue: "Hall B", capacity: 150, price_usd: 1800 }, operation_id: "op-reserve-hall-b-2026-11-14",
    domain_verified: true,
  });
  const late = await post(A, A.operator, "/jobs", {
    client_request_id: "dev-late", role_id: relocationRole, responsibility_id: relocateRsp, task: "Confirm shuttle times",
  });
  await sleep(DELAY);

  say("Relocation done: hand monitoring to a persistent CREAO role and retire the temporary lead");
  state = await get(A, "/organization");
  await propose(A, "dev-retire", {
    expected_org_revision: state.revision,
    trigger_evidence_ids: [obs.observation_id],
    rationale: "Temporary relocation work is complete; venue monitoring continues until the event",
    operations: [
      { op: "CreateRole", key: "monitor", name: "Venue monitor", charter: "Watch the booked venue for changes until the event",
        authority: { scopes: ["monitor.venue", "capability.venue_lookup"], budget_cents: 5000 },
        reports_to: { kind: "existing", role_id: coordinator }, provider: "creao", justification: "lifecycle" },
      { op: "RetireRole", role_id: relocationRole,
        handoffs: [{ responsibility_id: relocateRsp, to: { kind: "new", key: "monitor" } }] },
    ],
  });
  await sleep(DELAY);

  say("A late result from the retired role's epoch arrives and is fenced");
  try {
    await post(A, A.adapter, `/jobs/${late.job_id}/results`, {
      attempt_id: late.attempt_id, epoch: late.epoch, outcome: "succeeded", provider_status: "DEVSTUB_DONE",
      operation_id: "op-late-shuttle", domain_verified: true,
    });
    console.log("     unexpected: late result accepted");
  } catch (e) {
    console.log(`     rejected as expected: ${e.data?.error}`);
  }
  await sleep(DELAY);

  say("Org B, a separate organization, starts its own mission");
  const mb = await post(B, B.operator, "/missions", {
    goal: "Host a 300-person regional conference in March",
    constraints: ["Two-day event", "Budget $20,000"],
    success_checks: ["Venue for 300 reserved"],
    roles: [
      { key: "lead", name: "Conference lead", charter: "Owns the conference",
        authority: { scopes: ["mission.coordinate"], budget_cents: 200000 } },
      { key: "venue", name: "Venue operations", charter: "Owns venue logistics",
        authority: { scopes: ["capability.venue_lookup", "ledger.reserve_venue"], budget_cents: 50000 }, reports_to: "lead" },
    ],
    responsibilities: [
      { purpose: "Run the conference", owner: "lead" },
      { purpose: "Keep a venue for 300 booked", owner: "venue", trigger: "availability changes" },
    ],
  });
  const obsB = await post(B, B.agent, `/missions/${mb.mission_id}/observations`, {
    source: "operator", summary: "Need a way to check venue availability on a portal we have not used before",
  });
  await sleep(DELAY);

  say("Org B searches by its own need and installs the published capability (no synthesis)");
  const hits = await post(B, B.agent, "/capabilities/search", { query: "venue availability capacity portal" });
  console.log(`     top hit: ${hits[0]?.capability_id}@${hits[0]?.version} from ${hits[0]?.publisher}`);
  const instB = await post(B, B.operator, "/capability-installations", { capability_id: hits[0].capability_id, version: hits[0].version });
  console.log(`     synthesis calls: ${instB.synthesis_calls}`);
  await post(B, B.operator, `/capability-installations/${instB.installation_id}/acceptance`, { passed: true, report_ref: "local://org_b/acceptance/1" });
  let sb = await get(B, "/organization");
  const venueOps = roleByName(sb, "Venue operations").role_id;
  const venueRsp = Object.values(sb.responsibilities).find((r) => r.purpose.startsWith("Keep a venue")).responsibility_id;
  await propose(B, "dev-attach-b", {
    expected_org_revision: sb.revision,
    trigger_evidence_ids: [obsB.observation_id],
    rationale: "Venue operations adopts the verified capability",
    operations: [{ op: "AttachCapability", role: { kind: "existing", role_id: venueOps },
                   capability: { capability_id: hits[0].capability_id, version: hits[0].version } }],
  });
  await post(B, B.operator, "/capability-runs", {
    client_request_id: "dev-run-b", installation_id: instB.installation_id, role_id: venueOps,
    responsibility_id: venueRsp, input: { date: "2027-03-02", min_capacity: 300 },
  });

  console.log(`\nDone. Org A is at r${await rev(A)}, Org B at r${await rev(B)}. Open the console to inspect.`);
}

main().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
