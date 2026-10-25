// Independent #204 review guards. Retained package bytes are parsed/hashed only.
// No registry, provider, browser, executor or generated source is run.
import fs from "node:fs";
import { describe, expect, it } from "vitest";
import { canonicalJson } from "./digest";
import { buildExchange } from "./exchange";
import { intake } from "./intake";
import { buildLineage } from "./lineage";
import { decodePackageFile } from "./records";

const source = fs.readFileSync(new URL("../../../../docs/invention-lab/evidence/live-method-20261024-generated/method-package.json", import.meta.url), "utf8");
const original = decodePackageFile(new TextEncoder().encode(source));
function offer(text = source, publisher = "org_review_publisher") {
  const pkg = decodePackageFile(new TextEncoder().encode(text));
  return { publisher, publication: { reference: pkg.ref, package_json: text,
    intent: "public development method", applicability: ["rough-transport"] } };
}
function candidate() {
  return { publisher: "org_review_publisher", reference: original.ref,
    intent: "public development method", applicability: ["rough-transport"],
    runtime: original.runtime, protocol: original.protocol, dependencies: original.dependencies };
}
const read = (values: unknown[]) => intake(values.map((v, i) => ({ name: `record-${i}.json`, text: JSON.stringify(v) })));
const exchange = (values: unknown[]) => {
  const loaded = read(values);
  return buildExchange(loaded.exchange, buildLineage(loaded.records));
};
function changedSource(change: (pkg: { version: string; files: { content: string }[] }) => void) {
  const envelope = JSON.parse(source);
  change(envelope.package);
  return canonicalJson(envelope);
}

describe("review: exchange identities follow the registry contract", () => {
  it("control: one exact offer links bytes without creating acceptance", () => {
    const ex = exchange([offer()]);
    expect(ex.problems).toEqual([]);
    expect(ex.entries[0]!.fetched[0]!.package!.ref).toEqual(original.ref);
    expect(ex.entries[0]!.decisions).toEqual([]);
  });

  it("control: a new version or a different publisher is not an immutable-version conflict", () => {
    const next = changedSource((pkg) => { pkg.version = "0.2.0"; });
    const ex = exchange([offer(), offer(next), offer(source, "org_review_other")]);
    expect(ex.entries).toHaveLength(3);
    expect(ex.problems).toEqual([]);
  });

  it("reports two different source hashes for the same publisher, method ID and version", () => {
    const changed = changedSource((pkg) => { pkg.files[0]!.content += "\n"; });
    const ex = exchange([offer(), offer(changed)]);
    expect(ex.entries).toHaveLength(2); // Preserve both conflicting records for inspection.
    expect(ex.entries.every((e) => e.fetched[0]!.package !== null)).toBe(true);
    expect(ex.entries[0]!.reference.sha256).not.toBe(ex.entries[1]!.reference.sha256);
    expect(ex.problems.some((p) => /immutable|conflict|two different offers/i.test(p))).toBe(true);
  });

  it("refuses unknown fields nested in a publication's exact reference", () => {
    const value = offer();
    const bad = { ...value, publication: { ...value.publication,
      reference: { ...value.publication.reference, unexpected_provenance: "claimed" } } };
    expect(read([bad]).exchange.offers).toHaveLength(0);
  });

  for (const field of ["runtime", "protocol", "dependencies"] as const) {
    it(`refuses unknown fields in search ${field} references`, () => {
      const base = candidate();
      const invalid = { ...original.runtime, unexpected_provenance: "claimed" };
      const bad = { ...base, [field]: field === "dependencies" ? [invalid] : invalid };
      expect(read([{ candidates: [bad], truncated: false }]).exchange.searches).toHaveLength(0);
    });
  }
});
