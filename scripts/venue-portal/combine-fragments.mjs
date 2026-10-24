#!/usr/bin/env node
// Combine two organizations' public fragments for one scenario spec:
//   allowed_source_urls = A ∪ B, examples = A's (generation input),
//   invocation_input = B's (B's suite must contain exactly this input).
//
//   node scripts/venue-portal/combine-fragments.mjs --a data/portal/fragments/org-a.json \
//        --b data/portal/fragments/org-b.json > mission-fragment.json
import { readFileSync } from "node:fs";
import { parseArgs } from "node:util";

export function combine(a, b) {
  for (const [name, f] of [["A", a], ["B", b]]) {
    if (!Array.isArray(f?.allowed_source_urls) || !Array.isArray(f?.examples) || !f?.invocation_input) {
      throw new Error(`${name} is not a portal fragment`);
    }
  }
  const overlap = a.allowed_source_urls.filter((u) => b.allowed_source_urls.includes(u));
  if (overlap.length) throw new Error(`A and B share page URLs (${overlap[0]}); generate them in different namespaces`);
  return {
    note: "Public-safe. allowed_source_urls is A ∪ B; examples are A's; invocation_input is B's.",
    suite_a_id: a.suite_id,
    suite_b_id: b.suite_id,
    allowed_source_urls: [...a.allowed_source_urls, ...b.allowed_source_urls],
    examples: a.examples,
    invocation_input: b.invocation_input,
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const { values } = parseArgs({ options: { a: { type: "string" }, b: { type: "string" } } });
  if (!values.a || !values.b) {
    console.error("usage: combine-fragments.mjs --a A.json --b B.json");
    process.exit(2);
  }
  try {
    const read = (p) => JSON.parse(readFileSync(p, "utf8"));
    console.log(JSON.stringify(combine(read(values.a), read(values.b)), null, 2));
  } catch (e) {
    console.error(`error: ${e.message}`);
    process.exit(2);
  }
}
