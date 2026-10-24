// Independent PR88 controls: terminal installation never settles UNKNOWN work.
import { expect, it } from "vitest";
import { attention } from "./attention";
import type { Job, OrgState } from "./contracts";

it("keeps unknown check outcomes and ordinary invocation verification visible", () => {
  const cap = { capability_id: "reader", version: "1.0.0" };
  const state = {
    installations: { accepted: { installation_id: "accepted", capability: cap, status: "ACCEPTED" } },
  } as unknown as OrgState;
  const job = (job_id: string, task: string, status: Job["status"]) => ({
    job_id, task, status, capability: cap,
  }) as Job;
  const result = attention([], [
    job("unknown", "accept accepted with suite", "UNKNOWN"),
    job("invoke", "run accepted", "VERIFYING"),
    job("other-installation", "accept checking with suite", "VERIFYING"),
    job("settled", "accept accepted with suite", "VERIFYING"),
  ], [], state);
  expect(result.map((item) => [item.kind, item.refs[0]])).toEqual([
    ["unknown", "unknown"], ["waiting", "invoke"], ["waiting", "other-installation"],
  ]);
});
