import type { ExtractionContext, ExtractionOutcome } from './contract.js';

/** The forge replaces this module. This default never fabricates venue facts. */
export function extract(_html: string, context: ExtractionContext): ExtractionOutcome {
  return {
    status: 'unsupported',
    reason: 'unsupported_format',
    evidence: {
      source_url: context.source_url,
      snapshot_sha256: context.snapshot_sha256,
      locator: 'document',
    },
  };
}
