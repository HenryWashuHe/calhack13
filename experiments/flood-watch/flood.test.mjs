// node --test experiments/flood-watch/flood.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { assess, fetchSnapshot, observationFor, replaySnapshots, transition } from "./flood.mjs";

const fixture = JSON.parse(readFileSync(new URL("./fixtures/hunt-rising.json", import.meta.url)));
const gauge = (stage, cats = fixture.gauge.flood.categories) => ({
  lid: "TEST1", name: "Test River", flood: { categories: cats },
  status: { observed: { primary: stage, validTime: "2026-11-02T00:00:00Z" } },
});
const alerts = (...events) => ({ features: events.map((event, i) => ({ properties: { id: `a${i}`, event } })) });

test("classifies stage against official flood categories", () => {
  assert.equal(assess({ gauge: gauge(7), alerts: alerts() }).level, "normal");
  assert.equal(assess({ gauge: gauge(10) , alerts: alerts() }).level, "minor");
  assert.equal(assess({ gauge: gauge(12.5), alerts: alerts() }).level, "moderate");
  assert.equal(assess({ gauge: gauge(30), alerts: alerts() }).level, "major");
});

test("a flood warning is danger even below flood stage; a watch alone is not", () => {
  assert.equal(assess({ gauge: gauge(5), alerts: alerts("Flash Flood Warning") }).danger, true);
  assert.equal(assess({ gauge: gauge(5), alerts: alerts("Flash Flood Watch", "Wind Advisory") }).danger, false);
  assert.deepEqual(assess({ gauge: gauge(5), alerts: alerts("Wind Advisory") }).alerts, []);
});

test("missing readings or undefined stages are unknown, never normal", () => {
  assert.equal(assess({ gauge: gauge(-999), alerts: alerts() }).level, "unknown");
  const undefinedCats = { action: { stage: -9999 }, minor: { stage: -9999 } };
  assert.equal(assess({ gauge: gauge(4, undefinedCats), alerts: alerts() }).level, "unknown");
});

test("transitions: start, rise, steady, end", () => {
  const n = assess({ gauge: gauge(7), alerts: alerts() });
  const minor = assess({ gauge: gauge(10.5), alerts: alerts() });
  const mod = assess({ gauge: gauge(13), alerts: alerts() });
  assert.equal(transition(null, n), null);
  assert.equal(transition(n, minor), "flood_started");
  assert.equal(transition(minor, mod), "flood_rising");
  assert.equal(transition(mod, minor), null);
  assert.equal(transition(mod, n), "flood_ended");
});

test("the fixture timeline produces its expected events", () => {
  let prev = null;
  const seen = [];
  for (const snap of replaySnapshots(fixture)) {
    const a = assess(snap);
    const k = transition(prev, a);
    if (k) seen.push(k);
    prev = a;
  }
  assert.deepEqual(seen, fixture.expected);
});

test("observations are labeled by source and cite their evidence", () => {
  const [snap] = replaySnapshots(fixture);
  const fixtureObs = observationFor("flood_started", assess(snap), "fixture://x#0");
  assert.equal(fixtureObs.source, "flood-watch-fixture");
  assert.equal(fixtureObs.evidence_ref, "fixture://x#0");
  const live = observationFor("flood_started", assess({ gauge: { ...gauge(11), lid: "HNTT2" }, alerts: alerts() }));
  assert.equal(live.source, "noaa-nwps+nws-alerts");
  assert.equal(live.evidence_ref, "https://api.water.noaa.gov/nwps/v1/gauges/HNTT2");
  assert.match(live.summary, /^Flooding threat began: Test River/);
});

test("fetchSnapshot queries the gauge, then alerts at the gauge's location", async () => {
  const urls = [];
  const fake = async (url) => {
    urls.push(url);
    const body = url.includes("/gauges/") ? { ...fixture.gauge, status: { observed: { primary: 8 } } } : alerts();
    return { ok: true, json: async () => body };
  };
  const snap = await fetchSnapshot("HNTT2", fake);
  assert.equal(urls[0], "https://api.water.noaa.gov/nwps/v1/gauges/HNTT2");
  assert.equal(urls[1], "https://api.weather.gov/alerts/active?point=30.0699,-99.3217");
  assert.equal(assess(snap).level, "normal");
});
