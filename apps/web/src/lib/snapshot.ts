// Ordering guards for snapshot reads. REST loads can overlap (an SSE burst,
// a reconnect, a manual refresh) and complete out of order; the UI must never
// move backwards to an older snapshot, and a historical view must never show
// data for a revision other than the one requested.

import type { OrgState } from "./contracts";

/** Accepts snapshot responses only if they are not older than what is shown. */
export class SnapshotGate {
  private started = 0;
  private applied = 0;
  private revision = -1;

  /** Start a load; returns its generation. */
  begin(): number {
    return ++this.started;
  }

  /**
   * Whether a load's result may replace the current view. A response from an
   * earlier generation than one already applied, or one carrying an older
   * organization revision, is ignored.
   */
  accept(generation: number, revision: number): boolean {
    if (generation < this.applied || revision < this.revision) return false;
    this.applied = generation;
    this.revision = revision;
    return true;
  }

  /** Whether a load is the most recent one started (used for error reporting). */
  isLatest(generation: number): boolean {
    return generation === this.started;
  }

  /** Forget everything, e.g. when switching to another organization. */
  reset(): void {
    this.started = 0;
    this.applied = 0;
    this.revision = -1;
  }
}

/** A historical snapshot read, keyed by the revision it was requested for. */
export type HistoryEntry =
  | { revision: number; state: OrgState }
  | { revision: number; error: string };

export type HistoryView =
  | { kind: "live" }
  | { kind: "loading"; revision: number }
  | { kind: "error"; revision: number; message: string }
  | { kind: "ready"; revision: number; state: OrgState };

/** What to render for the selected revision. Entries for another revision are never shown. */
export function historyView(viewRevision: number | null, entry: HistoryEntry | null): HistoryView {
  if (viewRevision === null) return { kind: "live" };
  if (!entry || entry.revision !== viewRevision) return { kind: "loading", revision: viewRevision };
  if ("error" in entry) return { kind: "error", revision: viewRevision, message: entry.error };
  return { kind: "ready", revision: viewRevision, state: entry.state };
}
