// Review guard for #216 (Henry's Claude): a priority rule must not reach the
// host realm. The source's own top-level `this` is the contextified global,
// an object created in the host realm, so its constructor chain is the host's
// Function. These probes only set a marker on the host global; nothing is read
// or sent. Run: node --test scripts/iteration/families/review_host_realm.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { jobshopFamily } from "./jobshop.mjs";
import { binpackFamily } from "./binpack.mjs";

const families = [
  ["jobshop", jobshopFamily({ id: "jobshop", seed: "s" })],
  ["binpack", binpackFamily({ id: "binpack", seed: "s" })],
];

const probes = {
  "top-level this": "(() => { this.constructor.constructor('globalThis.__review216 = typeof process')(); return (x) => 0; })()",
  "arguments-free arrow at top level": "(this.constructor.constructor('globalThis.__review216 = typeof process')(), (x) => 0)",
};

for (const [name, family] of families) {
  for (const [label, source] of Object.entries(probes)) {
    test(`${name}: a rule cannot reach host process via ${label}`, () => {
      delete globalThis.__review216;
      const [i] = family.instances("realm", 1);
      family.score({ source }, i);
      assert.equal(globalThis.__review216, undefined, "the rule ran code in the host realm");
    });
  }
}
