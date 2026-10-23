import type { AgentBinding, Provider } from "../lib/contracts";
import { devStubTarget } from "../lib/graph";

type Tone = "ok" | "warn" | "bad" | "neutral" | "accent" | "faded";

const STATE_TONE: Record<string, Tone> = {
  ACTIVE: "ok",
  SUCCEEDED: "ok",
  COMMITTED: "ok",
  ACCEPTED: "ok",
  ASSIGNED: "accent",
  SATISFIED: "ok",
  READY: "accent",
  RUNNING: "accent",
  VERIFYING: "accent",
  VALIDATED: "accent",
  OPEN: "neutral",
  QUEUED: "neutral",
  SUBMITTING: "neutral",
  PROPOSED: "neutral",
  REQUESTED: "neutral",
  OFFERED: "accent",
  CHECKING: "warn",
  PROVISIONING: "warn",
  DRAINING: "warn",
  UNKNOWN: "warn",
  UNMET: "warn",
  FAILED: "bad",
  REJECTED: "bad",
  RESULT_REJECTED: "bad",
  PROVISIONING_FAILED: "bad",
  DECLINED: "bad",
  RETIRED: "faded",
  RELEASED: "faded",
  FENCED: "faded",
  COMPLETED: "faded",
};

export function StateChip({ state, title }: { state: string; title?: string }) {
  const tone = STATE_TONE[state] ?? "neutral";
  return (
    <span className={`chip ${tone}`} title={title}>
      {state.replace(/_/g, " ")}
    </span>
  );
}

export function ProviderBadge({ provider, binding }: { provider?: Provider; binding?: AgentBinding | null }) {
  const p = binding?.provider ?? provider ?? "internal";
  const stubFor = devStubTarget(binding);
  if (stubFor) {
    return (
      <span
        className="chip provider stub"
        style={{ ["--pc" as string]: "var(--p-dev_stub)" }}
        title="Development stub standing in for a provider. Not a live integration."
      >
        DEV STUB · for {stubFor}
      </span>
    );
  }
  return (
    <span className="chip provider" style={{ ["--pc" as string]: `var(--p-${p})` }}>
      {p}
    </span>
  );
}
