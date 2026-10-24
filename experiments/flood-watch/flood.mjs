// Flood watch: reads public NOAA river gauges (NWPS) and National Weather
// Service alerts, and turns meaningful changes into GENESIS observations.
// No dependencies. Pure functions are exported for tests.

export const NWPS = "https://api.water.noaa.gov/nwps/v1";
export const NWS = "https://api.weather.gov";
export const LEVELS = ["normal", "action", "minor", "moderate", "major"];
const HEADERS = {
  // api.weather.gov asks every client to identify itself.
  "User-Agent": "genesis-flood-watch (github.com/HenryWashuHe/genesis-history-experiment)",
  Accept: "application/geo+json, application/json",
};

// NWPS uses -999 / -9999 for "no value".
const num = (v) => (typeof v === "number" && Number.isFinite(v) && v > -900 ? v : null);
const rank = (level) => LEVELS.indexOf(level);

async function getJson(url, fetchImpl) {
  const res = await fetchImpl(url, { headers: HEADERS });
  if (!res.ok) throw new Error(`GET ${url} -> HTTP ${res.status}`);
  return res.json();
}

/** One live reading: the gauge (with its official flood stages) and active alerts at its location. */
export async function fetchSnapshot(lid, fetchImpl = fetch) {
  const gauge = await getJson(`${NWPS}/gauges/${encodeURIComponent(lid)}`, fetchImpl);
  const point = `${gauge.latitude.toFixed(4)},${gauge.longitude.toFixed(4)}`;
  const alerts = await getJson(`${NWS}/alerts/active?point=${point}`, fetchImpl);
  return { at: new Date().toISOString(), source: "live", gauge, alerts };
}

/** Classify one snapshot. Missing data is reported as unknown, never as safe. */
export function assess({ at, source = "live", gauge, alerts }) {
  const stage = num(gauge.status?.observed?.primary);
  const cats = gauge.flood?.categories ?? {};
  const thresholds = {};
  for (const level of LEVELS.slice(1)) {
    const s = num(cats[level]?.stage);
    if (s !== null) thresholds[level] = s;
  }
  let level = stage === null || Object.keys(thresholds).length === 0 ? "unknown" : "normal";
  if (level === "normal") {
    for (const l of LEVELS.slice(1)) if (thresholds[l] !== undefined && stage >= thresholds[l]) level = l;
  }
  const floodAlerts = (alerts?.features ?? [])
    .map((f) => f.properties ?? {})
    .filter((p) => /flood/i.test(p.event ?? ""))
    .map((p) => ({ id: p.id, event: p.event, headline: p.headline ?? p.event, ends: p.ends ?? p.expires ?? null }));
  const warning = floodAlerts.some((a) => /warning/i.test(a.event));
  return {
    at,
    source,
    lid: gauge.lid,
    name: gauge.name,
    downstream: gauge.downstreamLid || null,
    stage_ft: stage,
    observed_at: gauge.status?.observed?.validTime ?? null,
    level,
    thresholds,
    alerts: floodAlerts,
    danger: warning || rank(level) >= rank("minor"),
  };
}

/** What changed between two assessments, if anything worth telling the organization. */
export function transition(prev, cur) {
  if (!prev?.danger && cur.danger) return "flood_started";
  if (prev?.danger && !cur.danger) return "flood_ended";
  if (prev?.danger && cur.danger && rank(cur.level) > rank(prev.level)) return "flood_rising";
  return null;
}

export function describe(a) {
  const stage = a.stage_ft === null ? "no current reading" : `${a.stage_ft} ft`;
  const limit = a.thresholds[a.level] !== undefined ? ` (${a.level} flood stage ${a.thresholds[a.level]} ft)` : ` (${a.level})`;
  const alerts = a.alerts.length ? `; ${a.alerts.map((x) => x.event).join(", ")} in effect` : "; no flood alerts";
  return `${a.name} [${a.lid}]: ${stage}${limit}${alerts}`;
}

const SUMMARY = {
  flood_started: "Flooding threat began",
  flood_rising: "Flooding worsened",
  flood_ended: "Flooding threat ended",
};

/** Body for POST /api/v1/missions/{id}/observations. Fixture runs are labeled as fixtures. */
export function observationFor(kind, a, evidenceRef) {
  return {
    source: a.source === "live" ? "noaa-nwps+nws-alerts" : "flood-watch-fixture",
    summary: `${SUMMARY[kind]}: ${describe(a)}${a.observed_at ? `, observed ${a.observed_at}` : ""}`,
    evidence_ref: evidenceRef ?? `${NWPS}/gauges/${a.lid}`,
  };
}

/** Turn a recorded/synthetic timeline file into snapshots shaped like live ones. */
export function replaySnapshots(fixture) {
  return fixture.steps.map((step, i) => ({
    at: step.at,
    source: "fixture",
    step: i,
    gauge: {
      ...fixture.gauge,
      status: { observed: { primary: step.stage_ft, primaryUnit: "ft", validTime: step.at } },
    },
    alerts: { features: (step.alerts ?? []).map((event, j) => ({ properties: { id: `fixture-${i}-${j}`, event } })) },
  }));
}
