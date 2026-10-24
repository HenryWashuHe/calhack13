// Eric's Claude: static render of the retained first live attempt.
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it } from "vitest";
import { AttemptCard } from "./AttemptCard";
import { buildLineage } from "./lineage";
import { decodeCheckpointOutcome, decodeGenerationStatus } from "./records";

// Imported from the committed evidence directory, not copied.
import finalStatus from "../../../../docs/invention-lab/evidence/live-method-20261024/final-status.json";
import generationCheckpoint from "../../../../docs/invention-lab/evidence/live-method-20261024/generation.json";

const files: Record<string, unknown> = { "final-status.json": finalStatus, "generation.json": generationCheckpoint };
const evidence = (name: string): unknown => structuredClone(files[name]);

it("shows the failed attempt's cost, missing package and authority limit as recorded", () => {
  const status = decodeGenerationStatus(evidence("final-status.json"));
  const checkpoint = decodeCheckpointOutcome(evidence("generation.json"));
  const [attempt] = buildLineage({ generations: [{ status, checkpoint }] }).attempts;
  const html = renderToStaticMarkup(<AttemptCard attempt={attempt!} />);
  const text = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

  expect(text).toContain("org_example_a");
  expect(text).toContain("generation failed");
  expect(text).toContain("failed / protocol");
  expect(text).toContain("operator-observed cloud");
  expect(text).toContain("sampling provider_default");
  expect(text).toMatch(/input_tokens 3971 .*output_tokens 8192 .*wall_time_ms 62131/);
  expect(text).toContain("simulation_steps not recorded");
  expect(text).toContain("reached the output cap (not by itself a proven failure cause)");
  expect(text).toContain("package: none");
  expect(text).toContain(status.authority);
  // The record's own authority sentence denies improvement; nothing else may mention it.
  expect(text.replace(status.authority, "")).not.toMatch(/improv|succe/i);
});
