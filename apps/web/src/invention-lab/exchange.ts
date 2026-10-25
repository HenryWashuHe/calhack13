// Eric's Claude: method exchange records arranged per publisher-qualified exact
// method (docs/invention-lab/ui.md#method-exchange-records). Three states stay
// apart and none implies the next: discovered (a search hit, metadata only),
// fetched (an offer whose package bytes were checked here) and a local decision
// (a lab-adopt outcome recorded by a receiving organization). Source is linked
// to a method only through bytes that hash to its exact reference. Nothing here
// creates an adoption, selection or improvement edge.

import { canonicalSha256, sha256Hex } from "./digest";
import type { Intake } from "./intake";
import type { Lineage, MethodNode } from "./lineage";
import { decodePackageFile, refKey, type ExactRef, type MethodCandidate, type MethodOffer, type PackageFile } from "./records";

export interface Fetched {
  offer: MethodOffer;
  /** Decoded from the offer's exact bytes, only when they are its reference. */
  package: PackageFile | null;
  /** Why the bytes are not linked, or null when they match. */
  mismatch: string | null;
}

export interface ExchangeEntry {
  publisher: string;
  reference: ExactRef;
  /** Search hits naming this publisher and exact reference. */
  discovered: MethodCandidate[];
  fetched: Fetched[];
  /** The lineage method with this exact reference, linked only through matching fetched bytes. */
  node: MethodNode | null;
  /** Lab-adopt outcomes recorded for this exact method, by receiving organization. */
  decisions: { org_id: string; outcome: string }[];
}

export interface Exchange {
  entries: ExchangeEntry[];
  /** Search results that said more matches existed. */
  truncated: number;
  searches: number;
  problems: string[];
}

const short = (r: ExactRef) => `${r.id}@${r.version} ${r.sha256.slice(0, 12)}`;

function check(offer: MethodOffer): Fetched {
  const ref = offer.publication.reference;
  const bytes = new TextEncoder().encode(offer.publication.package_json);
  const sha = sha256Hex(bytes);
  if (sha !== ref.sha256)
    return { offer, package: null, mismatch: `its package bytes hash to ${sha.slice(0, 12)}, not the reference's ${ref.sha256.slice(0, 12)}` };
  let pkg: PackageFile;
  try {
    pkg = decodePackageFile(bytes);
  } catch (e) {
    return { offer, package: null, mismatch: `its package_json is not a readable method package (${(e as Error).message})` };
  }
  if (refKey(pkg.ref) !== refKey(ref))
    return { offer, package: null, mismatch: `its package is ${pkg.ref.schema} ${short(pkg.ref)}, not the reference's ${ref.schema} ${short(ref)}` };
  return { offer, package: pkg, mismatch: null };
}

/** Fields a fetched offer must share with a search hit for the same exact method. */
function disagreement(c: MethodCandidate, f: Fetched): string[] {
  const p = f.offer.publication;
  const same = (a: unknown, b: unknown) => canonicalSha256(a) === canonicalSha256(b);
  const out: string[] = [];
  if (c.intent !== p.intent) out.push("intent");
  if (!same(c.applicability, p.applicability)) out.push("applicability");
  if (f.package) {
    if (refKey(c.runtime) !== refKey(f.package.runtime)) out.push("runtime");
    if (refKey(c.protocol) !== refKey(f.package.protocol)) out.push("protocol");
    if (!same(c.dependencies.map(refKey), f.package.dependencies.map(refKey))) out.push("dependencies");
  }
  return out;
}

export function buildExchange(ex: Intake["exchange"], l: Lineage): Exchange {
  const entries = new Map<string, ExchangeEntry>();
  const problems: string[] = [];
  const entry = (publisher: string, reference: ExactRef) => {
    const key = `${publisher}\u0000${refKey(reference)}`;
    let e = entries.get(key);
    if (!e) {
      e = { publisher, reference, discovered: [], fetched: [], node: null, decisions: [] };
      entries.set(key, e);
    }
    return e;
  };

  for (const s of ex.searches)
    for (const c of s.candidates) {
      const e = entry(c.publisher, c.reference);
      if (!e.discovered.some((d) => canonicalSha256(d) === canonicalSha256(c))) e.discovered.push(c);
    }
  for (const offer of ex.offers) {
    const e = entry(offer.publisher, offer.publication.reference);
    if (e.fetched.some((f) => canonicalSha256(f.offer) === canonicalSha256(offer))) continue;
    if (e.fetched.length > 0)
      problems.push(`two different offers for ${offer.publisher}'s ${short(offer.publication.reference)}; a published version is immutable`);
    const f = check(offer);
    if (f.mismatch) problems.push(`offer for ${offer.publisher}'s ${short(offer.publication.reference)}: ${f.mismatch}; not linked`);
    e.fetched.push(f);
  }

  // The registry's immutable key is (publisher, method id, version), not the
  // digest: two references under one key are a conflict, whichever is right.
  // Both rows stay, so either can be inspected.
  const versions = new Map<string, Set<string>>();
  for (const e of entries.values()) {
    const k = `${e.publisher}\u0000${e.reference.id}\u0000${e.reference.version}`;
    versions.set(k, (versions.get(k) ?? new Set()).add(refKey(e.reference)));
  }
  for (const [k, refs] of versions)
    if (refs.size > 1) {
      const [publisher, id, version] = k.split("\u0000");
      const hashes = [...entries.values()]
        .filter((e) => e.publisher === publisher && e.reference.id === id && e.reference.version === version)
        .map((e) => e.reference.sha256.slice(0, 12))
        .sort();
      problems.push(
        `immutable version conflict: ${publisher}'s ${id}@${version} names ${refs.size} different references (${hashes.join(", ")}); a published version cannot change`,
      );
    }

  for (const e of entries.values()) {
    if (e.discovered.length > 1)
      problems.push(`search results describe ${e.publisher}'s ${short(e.reference)} in ${e.discovered.length} different ways`);
    for (const c of e.discovered)
      for (const f of e.fetched) {
        const fields = disagreement(c, f);
        if (fields.length > 0)
          problems.push(`offer for ${e.publisher}'s ${short(e.reference)} disagrees with its search hit on ${fields.join(", ")}`);
      }
    const key = refKey(e.reference);
    if (e.fetched.some((f) => f.package)) e.node = l.nodes.find((n) => n.key === key) ?? null;
    e.decisions = l.edges.flatMap((x) => (x.kind === "adoption" && x.to === key ? [{ org_id: x.org_id, outcome: x.outcome.outcome }] : []));
  }
  return {
    entries: [...entries.values()],
    truncated: ex.searches.filter((s) => s.truncated).length,
    searches: ex.searches.length,
    problems,
  };
}
