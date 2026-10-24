import { describe, expect, it } from "vitest";
import { elasticAnswer, lagging, recordId, searchPath } from "./evidenceSearch";

describe("evidence search helpers", () => {
  it("builds the search path with optional kinds", () => {
    expect(searchPath(" flooded hall ")).toBe("/search/evidence?q=flooded+hall&limit=10");
    expect(searchPath("venue", ["observation", "requirement"], 5)).toBe(
      "/search/evidence?q=venue&limit=5&kinds=observation%2Crequirement",
    );
  });

  it("detects a lagging index", () => {
    expect(lagging(null, 3)).toBe(true);
    expect(lagging(2, 3)).toBe(true);
    expect(lagging(3, 3)).toBe(false);
    expect(lagging(3, undefined)).toBe(false);
  });

  it("extracts record IDs from document IDs", () => {
    expect(recordId("org_a:observation:obs_1")).toBe("obs_1");
    expect(recordId("weird")).toBe("weird");
  });

  it("recognizes only Elastic specialist answers", () => {
    expect(elasticAnswer({ kind: "elastic_agent_answer", message: "m", tool_calls: ["t", 3] })).toEqual({
      message: "m",
      tools: ["t"],
    });
    expect(elasticAnswer({ message: "m" })).toBeNull();
    expect(elasticAnswer(null)).toBeNull();
  });
});
