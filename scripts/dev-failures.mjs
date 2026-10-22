#!/usr/bin/env node
// DEV-ONLY failure drills for the GENESIS kernel.
//
// Sends requests that a safe kernel must refuse or hold back, against the
// local dev stack, and checks each response. Every refusal is a real kernel
// decision; the actors are simulated with the dev tokens, and any provider
// binding involved is a DEV STUB. Refused changes, results and acceptance are
// recorded in the event log and listed in the console's Attention panel;
// route-level refusals (403) are not recorded and show only here.
//
// Usage: start `cargo run -p genesis-server -- dev`, then
//   node scripts/dev-failures.mjs [--org-a URL]
// Runs on a fresh stack or after scripts/dev-scenario.mjs. Exits 1 if any
// expectation fails. Each run adds a mission with two roles (the demo
// constitution allows ten active roles). See docs/FAILURE_DRILLS.md.

const args = Object.fromEntries(
  process.argv
    .slice(2)
    .join(" ")
    .split("--")
    .filter(Boolean)
    .map((a) => a.trim().split(/\s+/))
    .map(([k, v]) => [k, v ?? "true"]),
);
const A = { url: args["org-a"] ?? "http://127.0.0.1:7101", id: "org_a" };
A.operator = `${A.id}-operator`;
A.agent = `${A.id}-agent`;
A.adapter = `${A.id}-adapter`;
const RUN = Date.now().toString(36);

let step = 0;
const failures = [];
function say(msg) {
  step += 1;
  console.log(`\n[${String(step).padStart(2, "0")}] ${msg}`);
}
function ok(msg) {
  console.log(`     ok: ${msg}`);
}
function fail(msg) {
  failures.push(`[${String(step).padStart(2, "0")}] ${msg}`);
  console.log(`     FAILED EXPECTATION: ${msg}`);
}

async function call(token, method, path, body) {
  const res = await fetch(`${A.url}/api/v1${path}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = { error: "non_json", message: text.slice(0, 200) };
  }
  return { status: res.status, data };
}
const get = async (path) => {
  const r = await call(A.operator, "GET", path);
  if (r.status !== 200) throw new Error(`GET ${path} -> ${r.status} ${r.data?.error}`);
  return r.data;
};
const must = async (token, method, path, body) => {
  const r = await call(token, method, path, body);
  if (r.status >= 300) throw new Error(`${method} ${path} -> ${r.status} ${r.data?.error}: ${r.data?.message}`);
  return r.data;
};

/** Expect an HTTP refusal with this status and error code. */
function expectRefused(r, status, code, what) {
  if (r.status === status && r.data?.error === code) {
    ok(`${what}: refused with ${status} ${code} — ${r.data?.message ?? ""}`.trim());
  } else {
    fail(`${what}: expected ${status} ${code}, got ${r.status} ${r.data?.error ?? ""} ${r.data?.message ?? ""}`);
  }
}

async function waitChange(id) {
  for (let i = 0; i < 200; i++) {
    const c = await get(`/org-changes/${id}`);
    if (["COMMITTED", "FAILED", "REJECTED"].includes(c.status)) return c;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`change ${id} did not finish`);
}

const roleByName = (state, name) =>
  Object.values(state.roles).find((r) => r.name === name && r.state === "ACTIVE");
const openResponsibility = (state, roleId, purpose) => {
  const rsp = Object.values(state.responsibilities ?? {}).find((r) => r.purpose === purpose);
  if (!rsp) throw new Error(`responsibility "${purpose}" not found`);
  return rsp.responsibility_id;
};

async function dispatch(roleId, responsibilityId, task, requiredScope) {
  return must(A.operator, "POST", "/jobs", {
    client_request_id: `drill-${RUN}-${task.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`,
    role_id: roleId,
    responsibility_id: responsibilityId,
    task,
    ...(requiredScope ? { required_scope: requiredScope } : {}),
  });
}

function result(job, extra) {
  return {
    attempt_id: job.attempt_id,
    epoch: job.epoch,
    provider_status: "DEVSTUB_DONE",
    domain_verified: false,
    ...extra,
  };
}

async function main() {
  const health = await call(A.operator, "GET", "/health").catch(() => null);
  if (!health || health.status !== 200) {
    console.error(`Cannot reach ${A.url}. Start the stack: cargo run -p genesis-server -- dev`);
    process.exit(2);
  }
  if (!health.data.dev_stub_enabled) {
    console.error(`${A.url} does not run with development stubs. These drills are for the dev stack only.`);
    process.exit(2);
  }

  const lead = `Drill lead ${RUN}`;
  const desk = `Drill desk ${RUN}`;
  const deskDuty = `Answer attendee questions (${RUN})`;
  say("Setup: a small mission with a lead and a desk role");
  const m = await must(A.operator, "POST", "/missions", {
    goal: `Failure drills ${RUN}: the kernel must refuse unsafe requests`,
    constraints: ["Development stack only"],
    success_checks: ["Every unsafe request is refused or held back"],
    roles: [
      { key: "lead", name: lead, charter: "Owns the drills", authority: { scopes: ["mission.coordinate"], budget_cents: 1000 } },
      { key: "desk", name: desk, charter: "Answers attendees", authority: { scopes: ["attendees.inform"], budget_cents: 0 }, reports_to: "lead" },
    ],
    responsibilities: [
      { purpose: `Coordinate the drills (${RUN})`, owner: "lead" },
      { purpose: deskDuty, owner: "desk", trigger: "attendee questions", termination: "drills end" },
    ],
  });
  const obs = await must(A.agent, "POST", `/missions/${m.mission_id}/observations`, {
    source: "drill",
    summary: "Drill trigger",
  });
  let state = await get("/organization");
  const leadId = roleByName(state, lead).role_id;
  const deskId = roleByName(state, desk).role_id;
  const deskRsp = openResponsibility(state, deskId, deskDuty);
  ok(`mission ${m.mission_id}, org at r${state.revision}`);

  say("A proposal based on an old revision is rejected (optimistic concurrency)");
  let r = await call(A.agent, "POST", "/org-changes", {
    client_request_id: `drill-${RUN}-stale`,
    proposal: {
      expected_org_revision: state.revision - 1,
      trigger_evidence_ids: [obs.observation_id],
      rationale: "Desk should report to nobody (stale view)",
      operations: [{ op: "ChangeReportingEdge", role: { kind: "existing", role_id: deskId }, reports_to: null }],
    },
  });
  expectRefused(r, 409, "revision_conflict", "stale proposal");

  say("A role with authority outside the constitution is rejected");
  r = await call(A.agent, "POST", "/org-changes", {
    client_request_id: `drill-${RUN}-authority`,
    proposal: {
      expected_org_revision: state.revision,
      trigger_evidence_ids: [obs.observation_id],
      rationale: "Give a new role payment authority",
      operations: [
        {
          op: "CreateRole", key: "payer", name: `Payments ${RUN}`, charter: "Moves money",
          authority: { scopes: ["payments.transfer"], budget_cents: 0 },
          reports_to: { kind: "existing", role_id: leadId }, provider: "internal", justification: "authority",
        },
      ],
    },
  });
  expectRefused(r, 422, "missing_authority", "authority beyond the constitution");

  say("Retiring a role that still owns work, without a handoff, is rejected");
  r = await call(A.agent, "POST", "/org-changes", {
    client_request_id: `drill-${RUN}-retire`,
    proposal: {
      expected_org_revision: state.revision,
      trigger_evidence_ids: [obs.observation_id],
      rationale: "Retire the desk and forget its duty",
      operations: [{ op: "RetireRole", role_id: deskId, handoffs: [] }],
    },
  });
  expectRefused(r, 422, "validation_failed", "retirement that drops obligations");
  if (!(r.data?.details?.errors ?? []).some((e) => e.code === "retirement_drops_obligations")) {
    fail(`expected a retirement_drops_obligations error, got ${JSON.stringify(r.data?.details)}`);
  }

  say("The agent identity cannot dispatch work (operator-only route)");
  r = await call(A.agent, "POST", "/jobs", {
    client_request_id: `drill-${RUN}-agent-dispatch`, role_id: deskId, responsibility_id: deskRsp, task: "Anything",
  });
  expectRefused(r, 403, "missing_authority", "agent dispatching a job");

  say("Work outside the role's granted scope is refused");
  r = await call(A.operator, "POST", "/jobs", {
    client_request_id: `drill-${RUN}-scope`, role_id: deskId, responsibility_id: deskRsp,
    task: "Reserve a venue", required_scope: "ledger.reserve_venue",
  });
  expectRefused(r, 403, "missing_authority", "desk reserving a venue");

  say("Provider success without domain verification is not success");
  let job = await dispatch(deskId, deskRsp, "Answer FAQ batch 1");
  r = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, result(job, { outcome: "succeeded", operation_id: `drill-${RUN}-faq-1` }));
  let after = await get(`/jobs/${job.job_id}`);
  if (r.status === 202 && after.status === "VERIFYING") ok("job held in VERIFYING; no ledger effect");
  else fail(`expected 202 and VERIFYING, got ${r.status} and ${after.status}`);
  if ((await get("/ledger")).some((c) => c.operation_id === `drill-${RUN}-faq-1`)) fail("unverified result reached the ledger");

  say("A result for the wrong epoch is rejected");
  job = await dispatch(deskId, deskRsp, "Answer FAQ batch 2");
  r = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, result(job, { outcome: "succeeded", epoch: job.epoch + 1, domain_verified: true }));
  expectRefused(r, 409, "stale_epoch", "wrong-epoch result");
  after = await get(`/jobs/${job.job_id}`);
  if (after.status === "RESULT_REJECTED") ok("job is RESULT_REJECTED with the reason recorded");
  else fail(`expected RESULT_REJECTED, got ${after.status}`);

  say("A duplicate delivery has exactly one ledger effect");
  job = await dispatch(deskId, deskRsp, "Post the venue notice");
  const opId = `drill-${RUN}-notice`;
  const body = result(job, { outcome: "succeeded", domain_verified: true, operation_id: opId, output: { posted: true } });
  const first = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, body);
  const second = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, body);
  const rows = (await get("/ledger")).filter((c) => c.operation_id === opId).length;
  if (first.status === 200 && second.status === 200 && second.data?.duplicate === true && rows === 1) {
    ok("second delivery reported as duplicate; one ledger row");
  } else {
    fail(`expected 200, 200 duplicate and one row; got ${first.status}, ${second.status} duplicate=${second.data?.duplicate}, ${rows} rows`);
  }

  say("An unknown provider outcome is kept as UNKNOWN, not retried or counted");
  job = await dispatch(deskId, deskRsp, "Send the reminder email");
  r = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, result(job, { outcome: "unknown", provider_status: "DEVSTUB_CONNECTION_DROPPED" }));
  after = await get(`/jobs/${job.job_id}`);
  if (r.status === 202 && after.status === "UNKNOWN") ok("job is UNKNOWN and needs reconciliation");
  else fail(`expected 202 and UNKNOWN, got ${r.status} and ${after.status}`);

  say("After a handoff, a late result from the previous owner is fenced");
  job = await dispatch(deskId, deskRsp, "Update the FAQ page");
  state = await get("/organization");
  const handoff = await must(A.agent, "POST", "/org-changes", {
    client_request_id: `drill-${RUN}-handoff`,
    proposal: {
      expected_org_revision: state.revision,
      trigger_evidence_ids: [obs.observation_id],
      rationale: "The lead takes over attendee questions",
      operations: [{ op: "ReassignResponsibility", responsibility_id: deskRsp, from: deskId, to: { kind: "existing", role_id: leadId } }],
    },
  });
  const done = await waitChange(handoff.change_id);
  if (done.status !== "COMMITTED") fail(`handoff did not commit: ${done.status} ${JSON.stringify(done.errors)}`);
  r = await call(A.adapter, "POST", `/jobs/${job.job_id}/results`, result(job, { outcome: "succeeded", domain_verified: true, operation_id: `drill-${RUN}-late` }));
  if (r.status === 409 && ["stale_epoch", "result_fenced"].includes(r.data?.error)) {
    ok(`late result refused with ${r.data.error} — ${r.data.message}`);
  } else {
    fail(`expected 409 stale_epoch or result_fenced, got ${r.status} ${r.data?.error}`);
  }
  if ((await get("/ledger")).some((c) => c.operation_id === `drill-${RUN}-late`)) fail("late result reached the ledger");

  say("A capability that fails local acceptance cannot be attached");
  const version = `0.${Number.parseInt(RUN.slice(-4), 36) % 1000}.${Date.now() % 1000}`;
  const pub = await call(A.operator, "POST", "/capabilities/publish", {
    capability_id: "cap_drill_fixture",
    version,
    publisher: "org_a",
    source_digest: `d1${Date.now().toString(16)}`.padEnd(64, "0"),
    intent: "Drill fixture: summarize attendee questions",
    applicability_conditions: ["Drill only"],
    known_limitations: ["DEV FIXTURE: not built on Apify"],
    input_schema: { type: "object" },
    output_schema: { type: "object" },
    required_permissions: ["attendees.inform"],
    artifact: { actor_id: "devstub-actor-drill", build_id: "devstub-build", build_number: "0.0.0-dev" },
    responsibility_template: "Summarize attendee questions",
    local_configuration_schema: null,
    acceptance_suite_ref: "suite://drill/v1",
    verification: {
      passed: true,
      cases: [
        { name: "a", kind: "success", passed: true, detail: "dev fixture" },
        { name: "b", kind: "success", passed: true, detail: "dev fixture" },
        { name: "c", kind: "unsupported", passed: true, detail: "dev fixture" },
      ],
      tested_environment: "dev-failures fixture (not a live Apify build)",
      limitations: ["dev fixture"],
      oracle_ref: "oracle://drill/v1",
      verified_at: new Date().toISOString(),
    },
    provenance: ["org_a dev failure drills"],
    state: "APPROVED",
    published_at: null,
  });
  if (pub.status >= 300) {
    fail(`could not publish the drill fixture: ${pub.status} ${pub.data?.error} ${pub.data?.message}`);
  } else {
    const inst = await must(A.operator, "POST", "/capability-installations", { capability_id: "cap_drill_fixture", version });
    const acc = await call(A.operator, "POST", `/capability-installations/${inst.installation_id}/acceptance`, {
      passed: false,
      report_ref: `local://org_a/acceptance/drill-${RUN}`,
      reasons: ["Local fixture produced a different schema than the manifest"],
    });
    if (acc.status === 200 && acc.data?.installation?.status === "REJECTED") ok("installation REJECTED by local acceptance");
    else fail(`expected installation REJECTED, got ${acc.status} ${acc.data?.installation?.status ?? acc.data?.error}`);
    state = await get("/organization");
    r = await call(A.agent, "POST", "/org-changes", {
      client_request_id: `drill-${RUN}-attach`,
      proposal: {
        expected_org_revision: state.revision,
        trigger_evidence_ids: [obs.observation_id],
        rationale: "Attach the capability anyway",
        operations: [{ op: "AttachCapability", role: { kind: "existing", role_id: leadId }, capability: { capability_id: "cap_drill_fixture", version } }],
      },
    });
    expectRefused(r, 422, "incompatible_capability", "attaching a capability that failed acceptance");
  }

  console.log(
    failures.length === 0
      ? `\nAll drills behaved as expected. Org A is at r${(await get("/organization")).revision}; open the console's Attention panel.`
      : `\n${failures.length} expectation(s) failed:\n  ${failures.join("\n  ")}`,
  );
  process.exit(failures.length === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
