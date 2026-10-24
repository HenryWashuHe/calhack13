import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Generation } from "../lib/evolution";
import { EvolutionView } from "./EvolutionView";

const gens: Generation[] = [
  { revision: 1, label: "Founded", cause: "Run hack night", triggers: [], deltas: [{ kind: "role_added", text: "Coordinator", provider: "internal", devStub: false }], activeRoles: 1 },
  {
    revision: 4,
    label: "Reshaped",
    cause: "Hand monitoring to a persistent role",
    triggers: ["Main hall closed"],
    changeId: "chg_1",
    submittedBy: "planner",
    plannerModel: "claude-x",
    deltas: [
      { kind: "role_added", text: "Venue monitor", provider: "creao", devStub: true },
      { kind: "role_retired", text: "Relocation lead" },
    ],
    activeRoles: 2,
  },
];

describe("EvolutionView", () => {
  it("lists generations with cause, evidence and labeled dev stubs", () => {
    const html = renderToStaticMarkup(<EvolutionView gens={gens} selected={4} onSelect={() => {}} />);
    expect(html).toContain("Founded");
    expect(html).toContain("because</span> Main hall closed");
    expect(html).toContain("DEV STUB · for creao");
    expect(html).toContain("planner claude-x");
    expect(html).toContain("View r4");
    expect(html).toMatch(/evo-gen selected[^>]*>[\s\S]*r4/);
  });

  it("has an empty state", () => {
    expect(renderToStaticMarkup(<EvolutionView gens={[]} selected={null} onSelect={() => {}} />)).toContain(
      "No structural changes yet",
    );
  });
});
