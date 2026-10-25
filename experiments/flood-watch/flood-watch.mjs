#!/usr/bin/env node
// Flood watch driver. Reads live NOAA/NWS data (or a fixture timeline), prints
// each reading, and optionally tells a GENESIS organization what changed.
// See README.md in this folder for the demo walkthrough.
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { assess, describe, fetchSnapshot, observationFor, replaySnapshots, transition } from "./flood.mjs";

const HELP = `Usage: node experiments/flood-watch/flood-watch.mjs [options]
  --gauge LID        NOAA gauge to watch live (default HNTT2, Guadalupe River at Hunt, TX)
  --once             one live reading, then exit
  --interval SEC     seconds between live readings (default 300)
  --replay FILE      replay a fixture timeline instead of live data
  --delay MS         pause between replay steps (default 1500)
  --genesis URL      post observations to this GENESIS organization (e.g. http://127.0.0.1:7101)
  --org-id ID        that organization's id, for dev tokens (default org_a)
  --scripted         also propose the flood-response org changes (labeled scripted, not a planner)
  --org-b URL        after the flood, a second organization reuses the published capability
  --org-b-id ID      default org_b`;

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const opt = (name, dflt) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : dflt);
if (flag("help")) (console.log(HELP), process.exit(0));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const here = (p) => fileURLToPath(new URL(p, import.meta.url));

// ---- GENESIS HTTP helpers (same calls as scripts/dev-scenario.mjs) ----
const orgRef = (url, id) => url && { url, id, operator: `${id}-operator`, agent: `${id}-agent`, adapter: `${id}-adapter` };
async function call(o, token, method, path, body) {
  const res = await fetch(`${o.url}/api/v1${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${data?.error}: ${data?.message} ${JSON.stringify(data?.details ?? "")}`);
  return data;
}
const post = (o, token, path, body) => call(o, token, "POST", path, body);
const get = (o, path) => call(o, o.operator, "GET", path);
const roleByName = (s, name) => Object.values(s.roles).find((r) => r.name === name && r.state !== "RETIRED");

async function propose(o, id, proposal) {
  const rec = await post(o, o.agent, "/org-changes", { client_request_id: id, proposal });
  for (let i = 0; i < 200; i++) {
    const c = await get(o, `/org-changes/${rec.change_id}`);
    if (c.status === "COMMITTED") return (console.log(`    org change committed at r${c.committed_revision}`), c);
    if (["FAILED", "REJECTED"].includes(c.status)) throw new Error(`${id} ${c.status}: ${JSON.stringify(c.errors)}`);
    await sleep(150);
  }
  throw new Error(`${id} did not finish`);
}

async function ensureMission(o, body) {
  const s = await get(o, "/organization");
  const existing = Object.keys(s.missions ?? {})[0];
  if (existing) return existing;
  const m = await post(o, o.operator, "/missions", body);
  console.log(`    ${o.id}: mission ${m.mission_id} created`);
  return m.mission_id;
}

const missionA = (a) => ({
  goal: `Keep everyone at the riverside camp near ${a.name} safe through flood season`,
  constraints: ["Act only on official NWS alerts and NOAA flood stages", "A human approves every evacuation order"],
  success_checks: ["Everyone on site is notified within 10 minutes of a flood warning"],
  roles: [
    { key: "director", name: "Site director", charter: "Owns site safety and the season plan",
      authority: { scopes: ["mission.coordinate"], budget_cents: 200000 } },
    { key: "comms", name: "Alerts officer", charter: "Keeps staff and families informed",
      authority: { scopes: ["alerts.notify"], budget_cents: 20000 }, reports_to: "director" },
  ],
  responsibilities: [
    { purpose: "Coordinate site safety", owner: "director", trigger: "mission events", termination: "season ends" },
    { purpose: "Notify staff and families", owner: "comms", trigger: "safety changes", termination: "season ends" },
  ],
});

// ---- The flood-response organization ----
function responder(o, scripted) {
  let mission, lastObs, flood = {};
  return async function onChange(kind, a, evidenceRef) {
    mission ??= await ensureMission(o, missionA(a));
    const obs = await post(o, o.agent, `/missions/${mission}/observations`, observationFor(kind, a, evidenceRef));
    lastObs = obs.observation_id;
    console.log(`    observation ${lastObs} recorded`);
    if (kind === "flood_started") {
      const req = await post(o, o.agent, `/missions/${mission}/requirements`, {
        summary: `Respond to the flood threat at ${a.name}`, evidence: [lastObs],
        acceptance: "Evacuation plan issued and every person on site accounted for",
      });
      if (!scripted) return console.log("    requirement opened; the planner (if running) decides the change");
      const s = await get(o, "/organization");
      const c = await propose(o, `flood-start-${a.lid}`, {
        expected_org_revision: s.revision, trigger_evidence_ids: [lastObs],
        rationale: "A flood threat creates continuing response ownership that no existing role holds",
        budget_limit_cents: 160000,
        operations: [
          { op: "CreateRole", key: "flood", name: "Flood response lead",
            charter: "Plan the evacuation, account for everyone, and escalate as the river rises",
            authority: { scopes: ["flood.respond", "evacuation.plan", "alerts.notify"], budget_cents: 150000 },
            reports_to: { kind: "existing", role_id: roleByName(s, "Site director").role_id },
            provider: "elastic", justification: "independent_ownership" },
          { op: "AssignResponsibility", owner: { kind: "new", key: "flood" },
            target: { kind: "from_requirement", key: "respond", requirement_id: req.requirement_id,
                      purpose: "Respond to the flood", trigger: "river or alert changes", termination: "threat ends" } },
        ],
      });
      flood = { rsp: c.allocation.responsibilities.respond, role: roleByName(await get(o, "/organization"), "Flood response lead").role_id };
      console.log("    NEW ROLE: Flood response lead");
    } else if (kind === "flood_rising" && scripted && flood.role) {
      const job = await post(o, o.operator, "/jobs", {
        client_request_id: `flood-escalate-${a.at}`, role_id: flood.role, responsibility_id: flood.rsp,
        task: `Escalate: ${a.level} flood stage reached; move everyone to high ground`, required_scope: "evacuation.plan",
      });
      await post(o, o.adapter, `/jobs/${job.job_id}/results`, {
        attempt_id: job.attempt_id, epoch: job.epoch, outcome: "succeeded", provider_status: "DEVSTUB_DONE",
        output: { escalated_to: a.level, stage_ft: a.stage_ft }, operation_id: `op-escalate-${a.at}`, domain_verified: false,
      });
      console.log("    same structure: the existing Flood response lead escalates (no new role)");
    } else if (kind === "flood_ended" && scripted && flood.role) {
      const s = await get(o, "/organization");
      await propose(o, `flood-end-${a.lid}`, {
        expected_org_revision: s.revision, trigger_evidence_ids: [lastObs],
        rationale: "The threat has passed; continuing river watching moves to a persistent monitor",
        operations: [
          { op: "CreateRole", key: "monitor", name: "River monitor", charter: "Watch the river gauge until the season ends",
            authority: { scopes: ["monitor.river", "capability.river_gauge"], budget_cents: 5000 },
            reports_to: { kind: "existing", role_id: roleByName(s, "Site director").role_id },
            provider: "creao", justification: "lifecycle" },
          { op: "RetireRole", role_id: flood.role, handoffs: [{ responsibility_id: flood.rsp, to: { kind: "new", key: "monitor" } }] },
        ],
      });
      console.log("    Flood response lead RETIRED; River monitor holds the responsibility");
    }
  };
}

// ---- Self-check the kernel requires before publication: 2 success variants + 1 missing-data case ----
function selfCheck(fixture) {
  let prev = null;
  const seen = [];
  for (const snap of replaySnapshots(fixture)) {
    const k = transition(prev, (prev = assess(snap)));
    if (k) seen.push(k);
  }
  // Kerrville's real NWPS stages (action 7, minor 9, moderate 12, major 20); readings invented.
  const krr = (stage) => assess({ gauge: { lid: "KRRT2", name: "Guadalupe River at Kerrville", status: { observed: { primary: stage } },
    flood: { categories: { action: { stage: 7 }, minor: { stage: 9 }, moderate: { stage: 12 }, major: { stage: 20 } } } }, alerts: {} });
  const levels = [5, 9.5, 21].map((s) => krr(s).level).join(",");
  const missing = assess({ gauge: { lid: "X", name: "No data", status: { observed: { primary: -999 } }, flood: { categories: {} } }, alerts: {} });
  const cases = [
    { name: "hunt-rising timeline", kind: "success", passed: seen.join(",") === fixture.expected.join(","), detail: `events: ${seen.join(", ")}` },
    { name: "kerrville stage levels", kind: "success", passed: levels === "normal,minor,major", detail: `levels: ${levels}` },
    { name: "missing reading", kind: "unsupported", passed: missing.level === "unknown" && !missing.danger, detail: `level: ${missing.level}` },
  ];
  return { passed: cases.every((c) => c.passed), cases };
}

// ---- Sharing: A publishes the gauge reader, B (downstream) reuses it ----
async function shareWithDownstream(A, B, check) {
  const digest = createHash("sha256").update(readFileSync(here("./flood.mjs"))).digest("hex");
  console.log(`\n[share] ${A.id} publishes the river flood-stage reader (sha256 ${digest.slice(0, 12)}…)`);
  await post(A, A.operator, "/capabilities/publish", {
    capability_id: "cap_river_flood_stage", version: "0.1.0", publisher: A.id, source_digest: digest,
    intent: "Classify a NOAA river gauge reading against its official flood stages and active NWS flood alerts",
    applicability_conditions: ["NOAA NWPS gauge with defined flood categories"],
    known_limitations: ["Local Node module experiments/flood-watch/flood.mjs, not an Apify Actor",
      "Verification is a self-check on a synthetic timeline, not an independent oracle"],
    input_schema: { type: "object", properties: { lid: { type: "string" } }, required: ["lid"] },
    output_schema: { type: "object" }, required_permissions: ["capability.river_gauge"],
    artifact: { actor_id: "local-flood-watch", build_id: digest.slice(0, 12), build_number: "0.1.0-local" },
    responsibility_template: "Watch a river gauge for flood stage", local_configuration_schema: null,
    acceptance_suite_ref: "suite://flood-watch/hunt-rising",
    verification: {
      passed: check.passed, cases: check.cases,
      tested_environment: "flood-watch.mjs self-check (synthetic readings, real NOAA flood stages)",
      limitations: ["synthetic fixture"], oracle_ref: "oracle://flood-watch/hunt-rising", verified_at: new Date().toISOString(),
    },
    provenance: [`${A.id} flood-watch experiment`], state: "APPROVED", published_at: null,
  });
  const mission = await ensureMission(B, {
    goal: "Keep Kerrville riverside residents informed during floods",
    roles: [
      { key: "lead", name: "Emergency lead", charter: "Owns the town flood plan", authority: { scopes: ["mission.coordinate"], budget_cents: 100000 } },
      { key: "river", name: "River operations", charter: "Watches the Guadalupe at Kerrville",
        authority: { scopes: ["capability.river_gauge", "monitor.river"], budget_cents: 20000 }, reports_to: "lead" },
    ],
    responsibilities: [{ purpose: "Run the town flood plan", owner: "lead" }, { purpose: "Watch the river at Kerrville", owner: "river" }],
  });
  const obs = await post(B, B.agent, `/missions/${mission}/observations`, {
    source: "operator", summary: "Need to read flood stage for the Guadalupe River at Kerrville (KRRT2)",
  });
  const hits = await post(B, B.agent, "/capabilities/search", { query: "river gauge flood stage NWS alerts" });
  const hit = hits.find((h) => h.capability_id === "cap_river_flood_stage") ?? hits[0];
  if (!hit) throw new Error("registry returned no candidate");
  console.log(`[share] ${B.id} found ${hit.capability_id}@${hit.version} from ${hit.publisher}`);
  const inst = await post(B, B.operator, "/capability-installations", { capability_id: hit.capability_id, version: hit.version });
  console.log(`[share] ${B.id} installed it; synthesis calls: ${inst.synthesis_calls}`);
  await post(B, B.operator, `/capability-installations/${inst.installation_id}/acceptance`, { passed: true, report_ref: `local://${B.id}/flood-watch/acceptance` });
  const s = await get(B, "/organization");
  await propose(B, "flood-attach-b", {
    expected_org_revision: s.revision, trigger_evidence_ids: [obs.observation_id],
    rationale: "River operations adopts the upstream camp's verified gauge reader",
    operations: [{ op: "AttachCapability", role: { kind: "existing", role_id: roleByName(s, "River operations").role_id },
                   capability: { capability_id: hit.capability_id, version: hit.version } }],
  });
}

// ---- Main loop ----
const fixturePath = opt("replay");
const fixture = fixturePath && JSON.parse(readFileSync(fixturePath, "utf8"));
const A = orgRef(opt("genesis"), opt("org-id", "org_a"));
const B = orgRef(opt("org-b"), opt("org-b-id", "org_b"));
const onChange = A && responder(A, flag("scripted"));
const seen = [];
let prev = null;

async function handle(snap) {
  const a = assess(snap);
  const kind = transition(prev, a);
  prev = a;
  console.log(`${snap.at}  ${a.danger ? "DANGER" : "ok    "}  ${describe(a)}`);
  if (!kind) return;
  seen.push(kind);
  console.log(`  -> ${kind}`);
  const ref = snap.source === "fixture" ? `fixture://flood-watch/${fixture.gauge.lid}#step-${snap.step}` : undefined;
  if (onChange) await onChange(kind, a, ref);
}

if (fixture) {
  console.log(`Replaying ${fixturePath}\n  ${fixture.label}\n`);
  for (const snap of replaySnapshots(fixture)) (await handle(snap), await sleep(Number(opt("delay", 1500))));
  const check = selfCheck(fixture);
  console.log(`\nself-check: ${check.cases.map((c) => `${c.name} ${c.passed ? "pass" : "FAIL"}`).join("; ")}`);
  if (A && B) {
    if (!check.passed) throw new Error("self-check failed; not publishing");
    await shareWithDownstream(A, B, check);
  }
} else {
  const lid = opt("gauge", "HNTT2");
  for (;;) {
    try { await handle(await fetchSnapshot(lid)); } catch (e) { console.error(`reading failed (state unknown, not safe): ${e.message}`); }
    if (flag("once")) break;
    await sleep(Number(opt("interval", 300)) * 1000);
  }
}
