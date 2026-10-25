// Eric's Claude: method exchange records (MethodOffer / MethodSearchResults).
// Fixtures are built here from the retained A package on main; no registry,
// token, URL or credential is involved, and no source is executed.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { buildExchange } from "./exchange";
import { EXCHANGE_LIMIT, ExchangePanel } from "./ExchangePanel";
import { intake } from "./intake";
import { firstPassEvidenceFiles as evidenceFiles } from "./test-fixtures/first-pass";
import { buildLineage } from "./lineage";
import { LineagePanel } from "./LineagePanel";
import { decodePackageFile, type ExactRef } from "./records";

const root = fileURLToPath(new URL("../../../../docs/invention-lab/evidence/", import.meta.url));
const pkgPath = path.join(root, "live-method-20261024-generated/method-package.json");
const packageJson = fs.readFileSync(pkgPath, "utf8");
const pkg = decodePackageFile(new TextEncoder().encode(packageJson));
const reference: ExactRef = pkg.ref;
const A = "org_example_a";

function offer(over: Record<string, unknown> = {}, publication: Record<string, unknown> = {}) {
  return {
    publisher: A,
    publication: {
      reference,
      package_json: packageJson,
      intent: "screen-then-confirm cart search",
      applicability: ["public rough-transport development tasks"],
      ...publication,
    },
    ...over,
  };
}
function candidate(over: Record<string, unknown> = {}) {
  return {
    publisher: A,
    reference,
    intent: "screen-then-confirm cart search",
    applicability: ["public rough-transport development tasks"],
    runtime: pkg.runtime,
    protocol: pkg.protocol,
    dependencies: pkg.dependencies,
    ...over,
  };
}
const json = (name: string, v: unknown) => ({ name, text: JSON.stringify(v) });
const view = (files: { name: string; text: string; bytes?: Uint8Array }[]) => {
  const i = intake(files);
  const l = buildLineage(i.records);
  return { i, l, ex: buildExchange(i.exchange, l) };
};

describe("intake of exchange records", () => {
  it("reads an offer and a search result by their exact fields", () => {
    const { i } = view([json("offer.json", offer()), json("search.json", { candidates: [candidate()], truncated: false })]);
    expect(i.exchange.offers).toHaveLength(1);
    expect(i.exchange.searches).toHaveLength(1);
    expect(i.used.map((u) => u.note)).toEqual([
      "method offer (a registry response with source; not proof of its origin)",
      "method search results (public metadata, no source)",
    ]);
  });

  it("refuses unknown fields, as the registry's records do", () => {
    const { i } = view([
      json("a.json", offer({}, { signature: "x" })),
      json("b.json", { candidates: [{ ...candidate(), score: 1 }], truncated: false }),
    ]);
    expect(i.exchange.offers.length + i.exchange.searches.length).toBe(0);
    expect(i.skipped.map((s) => s.note)).toEqual([
      "malformed: offer.publication: unknown field signature",
      "malformed: search.candidates[0]: unknown field score",
    ]);
  });

  it("does not read a lab-exec run result as an adoption outcome", () => {
    const { i } = view([json("candidate.run.json", { outcome: "output_generated", usage: {} })]);
    expect(i.records.adoptions).toEqual([]);
    expect(i.skipped).toEqual([{ file: "candidate.run.json", note: "not a record type this view reads" }]);
  });
});

describe("discovered, fetched and decided stay apart", () => {
  it("shows a search hit as discovered only", () => {
    const { ex } = view([json("search.json", { candidates: [candidate()], truncated: true })]);
    expect(ex.entries).toHaveLength(1);
    expect(ex.entries[0]).toMatchObject({ publisher: A, fetched: [], node: null, decisions: [] });
    expect([ex.truncated, ex.problems]).toEqual([1, []]);
  });

  it("links fetched bytes to the lineage method only when they are its exact reference", () => {
    const { ex, l } = view([...evidenceFiles(), json("offer.json", offer()), json("search.json", { candidates: [candidate()], truncated: false })]);
    const e = ex.entries[0]!;
    expect(e.fetched[0]!.mismatch).toBeNull();
    expect(e.fetched[0]!.package!.ref).toEqual(reference);
    expect(e.node?.key).toBe(l.nodes.find((n) => n.ref.sha256 === reference.sha256)!.key);
    expect(ex.problems).toEqual([]);
  });

  it("does not link bytes that differ from the reference, and says why", () => {
    const tampered = packageJson.replace("def ", "def  ");
    const { ex } = view([...evidenceFiles(), json("offer.json", offer({}, { package_json: tampered }))]);
    const e = ex.entries[0]!;
    expect([e.fetched[0]!.package, e.node]).toEqual([null, null]);
    expect(ex.problems[0]).toMatch(/package bytes hash to [0-9a-f]{12}, not the reference's 8215c3679a06; not linked$/);
  });

  it("does not link bytes whose package names another method under the same hash", () => {
    const { ex } = view([json("offer.json", offer({}, { reference: { ...reference, id: "renamed" } }))]);
    expect(ex.entries[0]!.fetched[0]!.package).toBeNull();
    expect(ex.problems[0]).toContain("its package is development-method-source/1 generated-method@0.1.0");
  });

  it("reports an offer that disagrees with its search hit", () => {
    const other = { ...pkg.runtime, sha256: "0".repeat(64) };
    const { ex } = view([json("offer.json", offer()), json("search.json", { candidates: [candidate({ intent: "something else", runtime: other })], truncated: false })]);
    expect(ex.problems).toEqual([
      `offer for ${A}'s generated-method@0.1.0 8215c3679a06 disagrees with its search hit on intent, runtime`,
    ]);
  });

  it("keeps the same exact method from two publishers as two entries", () => {
    const { ex } = view([json("a.json", offer()), json("b.json", offer({ publisher: "org_example_b" }))]);
    expect(ex.entries.map((e) => e.publisher)).toEqual([A, "org_example_b"]);
  });

  it("reports two different offers for one publisher and version", () => {
    const { ex } = view([json("a.json", offer()), json("b.json", offer({}, { intent: "changed" }))]);
    expect(ex.problems[0]).toContain("two different offers");
  });

  it("shows local decisions only from lab-adopt records, and creates none", () => {
    const decided = view([
      json("offer.json", offer()),
      json("adopt.json", { outcome: "not_applicable", reason: "1/6 vs 2/6" }),
    ]);
    // A lab-adopt outcome without an installation names no receiver: it is listed, not paired.
    expect(decided.ex.entries[0]!.decisions).toEqual([]);
    const installed = view([
      json("offer.json", offer()),
      json("adopt.json", {
        outcome: "adopted",
        installation: { org_id: "org_example_b", method: reference, verdict: { verdict: "accepted", candidate: 2, control: 1, of: 3 } },
      }),
    ]);
    expect(installed.ex.entries[0]!.decisions).toEqual([{ org_id: "org_example_b", outcome: "adopted" }]);
    const withOffer = view([...evidenceFiles(), json("offer.json", offer())]);
    const without = view(evidenceFiles());
    expect(withOffer.l.edges).toEqual(without.l.edges);
  });
});

describe("the exchange panel", () => {
  it("states what the records are and keeps the three states in separate columns", () => {
    const { ex, l } = view([...evidenceFiles(), json("offer.json", offer()), json("search.json", { candidates: [candidate()], truncated: true })]);
    const html = renderToStaticMarkup(<LineagePanel lineage={l} exchange={ex} />);
    const text = html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'").replace(/&quot;/g, '"').replace(/\s+/g, " ");
    expect(text).toContain("Method exchange records");
    expect(text).toContain(EXCHANGE_LIMIT);
    expect(text).toContain("1 search result supplied, 1 truncated (more matches existed)");
    expect(text).toContain(`${A}: generated-method@0.1.0 8215c3679a06`);
    expect(text).toContain("search hit (metadata only, no source)");
    expect(text).toContain("bytes are exactly this reference (checked in this browser)");
    expect(text).toContain("same exact method as in the lineage above");
    expect(text).toContain("none recorded");
    // The exchange section itself makes no claim beyond the records (the
    // lineage panel around it has its own, negated, limit wording).
    const panel = renderToStaticMarkup(<ExchangePanel exchange={ex} />);
    expect(panel).toContain(EXCHANGE_LIMIT);
    expect(panel.replace(EXCHANGE_LIMIT, "")).not.toMatch(/improve|better|recommend/i);
  });

  it("renders nothing without exchange records", () => {
    const { ex } = view([]);
    expect(renderToStaticMarkup(<ExchangePanel exchange={ex} />)).toBe("");
  });
});
