// Fetch.ai: the channel between organizations. An exported bundle becomes a method offer that an agent
// can carry as one chat message, and the same offer becomes an ordinary kernel observation (the body the
// ACP mission bridge already posts: source, summary, evidence_ref), so the receiving organization's
// planner sees "a verified method is on offer" through the same door as every other piece of evidence.
// The receiver never trusts the sender's score: `adopt` re-runs the package on the receiver's own panel.
export const PROTOCOL = "genesis-method-offer/1";

export function toOffer(bundle, { from, to = null, bundle_ref = null } = {}) {
  if (!bundle || !bundle.package || !bundle.ledger_sha256 || !bundle.seal) throw new Error("toOffer needs an exported bundle");
  return {
    protocol: PROTOCOL, from: from || bundle.exported_by, to, family: bundle.family,
    package: { sha256: bundle.package.sha256, origin: bundle.package.origin, generation: bundle.package.generation, bytes: bundle.package.source.length },
    claimed: { success_count: bundle.success_count, selection_plan: bundle.plan, note: "sender's selection score; receivers re-verify on their own instances" },
    lineage: bundle.lineage.map((l) => l.sha256), ledger_sha256: bundle.ledger_sha256, records: bundle.ledger.records.length, seal: bundle.seal, bundle_ref,
  };
}

/** The chat text an agent sends: a one-line human summary followed by the JSON offer. */
export function toChatText(offer) {
  const head = `Method offer from ${offer.from}: family ${offer.family}, package ${offer.package.sha256.slice(0, 10)} (generation ${offer.package.generation}), sender's score ${offer.claimed.success_count}, ledger ${offer.ledger_sha256.slice(0, 10)} over ${offer.records} records. Verify before adopting.`;
  return `${head}\n${JSON.stringify(offer)}`;
}

/** Parses an offer back out of chat text; null when the text carries none. */
export function fromChatText(text) {
  const line = String(text || "").split("\n").find((l) => l.trim().startsWith("{"));
  if (!line) return null;
  try { const o = JSON.parse(line); return o && o.protocol === PROTOCOL ? o : null; } catch { return null; }
}

/** The kernel observation body the mission bridge posts; evidence_ref points at the sealed bundle. */
export function toObservation(offer, { source = "acp-mission-agent", evidence_ref } = {}) {
  return { source, summary: toChatText(offer).split("\n")[0], evidence_ref: evidence_ref || offer.bundle_ref || `ledger:${offer.ledger_sha256}` };
}
