// Eric's Claude: an arithmetic ledger over generation attempts
// (docs/invention-lab/ui.md#attempt-ledger). Failed and unknown attempts count.
// A total is exact only when every attempt measured that dimension; otherwise
// it is a lower bound. This is not a predeclared combined budget, a per-success
// cost or a comparison.

import type { Tier } from "./claims";
import type { Attempt } from "./lineage";
import { USAGE_DIMENSIONS, type UsageDimension } from "./records";

export interface LedgerTotal {
  /** Sum of measured values only. */
  measured: number;
  /** Attempts that measured this dimension. */
  measuredCount: number;
  /** Attempts whose value is unknown, unrecognized, not recorded, or that have no completion. */
  missingCount: number;
}

export interface Ledger {
  attempts: number;
  byPhase: Record<string, number>;
  packages: number;
  tiers: Tier[];
  totals: Record<UsageDimension, LedgerTotal>;
}

export function buildLedger(attempts: Attempt[]): Ledger {
  const totals = {} as Record<UsageDimension, LedgerTotal>;
  for (const d of USAGE_DIMENSIONS) totals[d] = { measured: 0, measuredCount: 0, missingCount: 0 };
  const byPhase: Record<string, number> = {};
  const tiers: Tier[] = [];
  let packages = 0;
  for (const a of attempts) {
    byPhase[a.status.phase] = (byPhase[a.status.phase] ?? 0) + 1;
    if (a.status.package) packages += 1;
    if (!tiers.includes(a.tier)) tiers.push(a.tier);
    for (const d of USAGE_DIMENSIONS) {
      const u = a.status.usage?.[d];
      if (u?.kind === "measured") {
        totals[d].measured += u.value;
        totals[d].measuredCount += 1;
      } else {
        totals[d].missingCount += 1;
      }
    }
  }
  return { attempts: attempts.length, byPhase, packages, tiers, totals };
}

/** "7999", "≥ 7999 (1 of 2 not measured)", or "not recorded". */
export function totalText(t: LedgerTotal): string {
  if (t.measuredCount === 0) return t.missingCount === 0 ? "0" : "not recorded";
  if (t.missingCount === 0) return String(t.measured);
  return `≥ ${t.measured} (${t.missingCount} of ${t.measuredCount + t.missingCount} not measured)`;
}
