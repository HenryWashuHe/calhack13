// Eric's Claude: lineage over the retained first live attempt plus synthetic
// revision/adoption records.
import { describe, expect, it } from "vitest";
import { buildLineage } from "./lineage";
import {
  decodeAdoptionOutcome,
  decodeCheckpointOutcome,
  decodeGenerationStatus,
  decodeRevisionObservation,
  refKey,
  type ExactRef,
  type GenerationStatus,
} from "./records";
import finalStatus from "../../../../docs/invention-lab/evidence/live-method-20261024/final-status.json";
import generationCheckpoint from "../../../../docs/invention-lab/evidence/live-method-20261024/generation.json";

const liveStatus = () => decodeGenerationStatus(structuredClone(finalStatus));
const liveCheckpoint = () => decodeCheckpointOutcome(structuredClone(generationCheckpoint));

const ref = (id: string, version: string, sha: string): ExactRef => ({
  domain: "genesis.development_method_source",
  schema: "development-method-source/1",
  id,
  version,
  sha256: sha.repeat(64),
});
const revision = (parent: ExactRef, child: ExactRef, source_changed: boolean) =>
  decodeRevisionObservation({
    schema: "genesis-method-revision-observation/1",
    parent,
    child,
    org_id: "org_b",
    operation_id: "op-b",
    transport: "loopback_fixture",
    source_changed,
  });
const installed = (org_id: string, method: ExactRef) =>
  decodeAdoptionOutcome({
    outcome: "adopted",
    installation: { org_id, method, verdict: { verdict: "accepted", candidate: 3, control: 1, of: 3 } },
  });

function generated(op: string, pkg: ExactRef): GenerationStatus {
  return decodeGenerationStatus({
    operation_id: op,
    org_id: "org_b",
    phase: "generated",
    settings: { model: "m", max_tokens: 100, timeout_ms: 1, sampling: "provider_default" },
    plan_sha256: "p",
    request_sha256: "r",
    wire_sha256: "w",
    transport: "loopback_fixture",
    actual_model: "m",
    usage: { model_calls: { kind: "measured", value: 1 }, output_tokens: { kind: "measured", value: 40 } },
    package: pkg,
    authority: "none",
  });
}

describe("first live attempt", () => {
  it("joins its checkpoint outcome, reaches the output cap and yields no node or edge", () => {
    const l = buildLineage({ generations: [{ status: liveStatus(), checkpoint: liveCheckpoint() }] });
    expect(l.problems).toEqual([]);
    expect(l.attempts[0]!.outcome).toEqual({ kind: "failed", reason: "protocol" });
    expect(l.attempts[0]!.tier).toBe("operator_observed_cloud");
    expect(l.attempts[0]!.reachedOutputCap).toBe(true);
    expect(l.nodes).toEqual([]);
    expect(l.edges).toEqual([]);
  });

  it("does not use a checkpoint that belongs to a different request", () => {
    const checkpoint = { ...liveCheckpoint(), request_sha256: "other" };
    const l = buildLineage({ generations: [{ status: liveStatus(), checkpoint }] });
    expect(l.attempts[0]!.outcome).toBeNull();
    expect(l.problems[0]).toMatch(/does not match/);
  });
});

describe("lineage", () => {
  const parent = ref("a-method", "1.0.0", "a");
  const child = ref("b-method", "2.0.0", "b");

  it("links an exact revision and each adoption outcome with its pairing source", () => {
    const l = buildLineage({
      generations: [{ status: generated("op-b", child) }],
      revisions: [revision(parent, child, false)],
      adoptions: [
        { receiver_org: "org_a", offer: child, outcome: installed("org_a", child) },
        { receiver_org: "org_c", offer: child, outcome: decodeAdoptionOutcome({ outcome: "not_applicable", reason: "0/3" }) },
        { receiver_org: "org_d", offer: parent, outcome: decodeAdoptionOutcome({ outcome: "unknown", reason: "scoring missing" }) },
      ],
    });
    expect(l.problems).toEqual([]);
    expect(l.nodes.map((n) => n.ref.id).sort()).toEqual(["a-method", "b-method"]);
    expect(l.edges.map((e) => e.kind)).toEqual(["generated", "revised_from", "adoption", "adoption", "adoption"]);
    expect(l.edges.flatMap((e) => (e.kind === "adoption" ? [e.pairing] : []))).toEqual(["record", "host", "host"]);
  });

  it("reports a revision child that differs from its operation's package", () => {
    const l = buildLineage({ generations: [{ status: generated("op-b", parent) }], revisions: [revision(parent, child, true)] });
    expect(l.problems[0]).toMatch(/revision child differs/);
  });

  it("reports an installation that contradicts the supplied receiver or offer", () => {
    const l = buildLineage({ generations: [], adoptions: [{ receiver_org: "org_a", offer: child, outcome: installed("org_a", parent) }] });
    expect(l.problems[0]).toMatch(/does not match/);
    const edge = l.edges[0]!;
    expect(edge.kind === "adoption" && edge.to).toBe(refKey(parent));
  });

  it("treats the same id/version with a different digest as a different method", () => {
    const l = buildLineage({ generations: [], revisions: [revision(ref("m", "1", "a"), ref("m", "1", "c"), true)] });
    expect(l.nodes).toHaveLength(2);
  });
});
