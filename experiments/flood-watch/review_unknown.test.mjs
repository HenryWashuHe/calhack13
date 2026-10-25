// Independent review: pure assessment/transition checks; no feeds or driver execution.
import { test } from "node:test";
import assert from "node:assert/strict";
import { assess, transition } from "./flood.mjs";

const categories = {
  action: { stage: 9 }, minor: { stage: 10 },
  moderate: { stage: 12 }, major: { stage: 22 },
};
const reading = (stage, thresholds = categories) => assess({
  source: "fixture",
  gauge: {
    lid: "TEST1", name: "Synthetic review gauge",
    flood: { categories: thresholds },
    status: { observed: { primary: stage } },
  },
  alerts: { features: [] },
});

test("missing reading must not deactivate an existing flood response", () => {
  const active = reading(13);
  const unavailable = reading(-999);
  assert.equal(active.danger, true);
  assert.equal(unavailable.level, "unknown");
  assert.notEqual(transition(active, unavailable), "flood_ended");
});

test("known zero reading without thresholds must remain unknown, not clear a flood", () => {
  const active = reading(13);
  const unclassifiable = reading(0, {});
  assert.equal(active.danger, true);
  assert.equal(unclassifiable.stage_ft, 0);
  assert.equal(unclassifiable.level, "unknown");
  assert.notEqual(transition(active, unclassifiable), "flood_ended");
});

test("known normal reading with thresholds can end the response", () => {
  const active = reading(13);
  const normal = reading(7);
  assert.equal(active.danger, true);
  assert.equal(normal.level, "normal");
  assert.equal(normal.danger, false);
  assert.equal(transition(active, normal), "flood_ended");
});
