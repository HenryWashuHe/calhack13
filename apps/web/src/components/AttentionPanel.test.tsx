import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Api } from "../lib/api";
import type { OrgChangeRecord } from "../lib/contracts";
import { AttentionPanel } from "./AttentionPanel";

// PR #28 review: after Attach the operation moves to awaiting_readiness. The
// panel must keep an Abandon path (saga::abandon_operation accepts that
// state) and must not offer Attach again for an already-bound operation.

const api = {} as Api;

function change(state: string, extra: Record<string, unknown> = {}): OrgChangeRecord {
  return {
    change_id: "chg_1",
    status: "PROVISIONING",
    operations: {
      monitor: { provider: "creao", operation_id: "creao-role-1", state, updated_at: "2026-10-22T00:00:00Z", ...extra },
    },
  } as unknown as OrgChangeRecord;
}

function render(c: OrgChangeRecord): string {
  return renderToStaticMarkup(<AttentionPanel api={api} changes={[c]} jobs={[]} events={[]} state={null} />);
}

describe("AttentionPanel recovery actions", () => {
  it("offers Attach and Abandon for an unknown creation outcome", () => {
    const html = render(change("needs_reconciliation"));
    expect(html).toContain("Attach agent");
    expect(html).toContain("Abandon");
    expect(html).toContain("provider agent ID");
  });

  it("keeps only Abandon after an operator attached the agent", () => {
    const html = render(
      change("awaiting_readiness", { native_status: "operator_attached", attention: "no readiness verifier" }),
    );
    expect(html).toContain("Abandon");
    expect(html).not.toContain("Attach agent");
    expect(html).not.toContain("provider agent ID");
  });

  it("keeps only Abandon while a provider-created agent awaits readiness", () => {
    const html = render(change("awaiting_readiness"));
    expect(html).toContain("Abandon");
    expect(html).not.toContain("Attach agent");
  });
});
