import { createHash } from 'node:crypto';
import type { ActorInput, EvidenceRef, ExtractionContext, ExtractionOutcome, Extractor } from './contract.js';

export interface SourcePolicy { readonly urls: readonly string[] }
export interface RuntimeLimits { readonly maxBytes: number; readonly timeoutMs: number }
const DEFAULT_LIMITS: RuntimeLimits = Object.freeze({ maxBytes: 2 * 1024 * 1024, timeoutMs: 10_000 });
const MAX_OUTPUT_BYTES = 1024 * 1024;

function object(value: unknown, fields: readonly string[], path: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${path}: expected object`);
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== fields.length || fields.some((field) => !Object.hasOwn(record, field))) {
    throw new Error(`${path}: incorrect fields`);
  }
  return record;
}
function text(value: unknown, path: string, max = 1024): asserts value is string {
  if (typeof value !== 'string' || value.length === 0 || value.length > max || value.trim() !== value) throw new Error(`${path}: invalid string`);
}
function integer(value: unknown, path: string, min = 0, max = Number.MAX_SAFE_INTEGER): asserts value is number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < min || value > max) throw new Error(`${path}: invalid integer`);
}
function instant(value: unknown, path: string): number {
  text(value, path, 64);
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?(Z|[+-]\d{2}:\d{2})$/.exec(value);
  if (!match) throw new Error(`${path}: explicit RFC3339 offset required`);
  const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number);
  if (year === undefined || month === undefined || day === undefined || hour === undefined || minute === undefined || second === undefined
    || month < 1 || month > 12 || day < 1 || day > new Date(Date.UTC(year, month, 0)).getUTCDate()
    || hour > 23 || minute > 59 || second > 59 || !Number.isFinite(Date.parse(value))) throw new Error(`${path}: invalid timestamp`);
  return Date.parse(value);
}
function window(value: unknown, path: string): void {
  const candidate = object(value, ['start', 'end'], path);
  if (instant(candidate.start, `${path}/start`) >= instant(candidate.end, `${path}/end`)) throw new Error(`${path}: nonpositive window`);
}
function canonicalUrl(value: unknown, testOnlyAllowLoopbackHttp: boolean): string {
  text(value, 'source_url', 4096);
  const url = new URL(value);
  const testHttp = testOnlyAllowLoopbackHttp && url.protocol === 'http:' && ['127.0.0.1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !testHttp) || url.username || url.password || url.hash || url.href !== value) {
    throw new Error('source_url: canonical HTTPS URL required');
  }
  return value;
}

/** Only the trusted entrypoint calls this with build-time configuration.
 * The loopback option is for native tests; main.ts never enables it. */
export function createSourcePolicy(value: unknown, testOnlyAllowLoopbackHttp = false): SourcePolicy {
  const config = object(value, ['urls'], 'source policy');
  if (!Array.isArray(config.urls) || config.urls.length > 100) throw new Error('source policy: invalid URL list');
  const urls = config.urls.map((url: unknown) => canonicalUrl(url, testOnlyAllowLoopbackHttp));
  if (new Set(urls).size !== urls.length) throw new Error('source policy: duplicate URL');
  return Object.freeze({ urls: Object.freeze(urls) });
}

export function validateInput(value: unknown, policy: SourcePolicy): ActorInput {
  const input = object(value, ['source_url', 'attendees', 'requested_window'], 'input');
  text(input.source_url, 'input/source_url', 4096);
  if (!policy.urls.includes(input.source_url)) throw new Error('input/source_url: URL is outside the built allowlist');
  integer(input.attendees, 'input/attendees', 1, 4294967295);
  window(input.requested_window, 'input/requested_window');
  // Copy before freezing so external input mutation cannot change context later.
  return JSON.parse(JSON.stringify(input)) as ActorInput;
}

async function fetchSnapshot(url: string, limits: RuntimeLimits): Promise<{ html: string; sha256: string }> {
  integer(limits.maxBytes, 'maxBytes', 1, DEFAULT_LIMITS.maxBytes);
  integer(limits.timeoutMs, 'timeoutMs', 1, DEFAULT_LIMITS.timeoutMs);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), limits.timeoutMs);
  try {
    const response = await fetch(url, {
      redirect: 'error', signal: controller.signal,
      headers: { accept: 'text/html, application/xhtml+xml', 'accept-encoding': 'identity' },
    });
    if (!response.ok) throw new Error(`source fetch: HTTP ${response.status}`);
    const contentType = response.headers.get('content-type')?.toLowerCase() ?? '';
    if (!/^(text\/html|application\/xhtml\+xml)(?:\s*;|$)/.test(contentType)) throw new Error('source fetch: HTML content type required');
    const charset = /charset\s*=\s*["']?([^\s;"']+)/i.exec(contentType)?.[1];
    if (charset && !['utf-8', 'utf8'].includes(charset)) throw new Error('source fetch: UTF-8 required');
    const encoding = response.headers.get('content-encoding');
    if (encoding && encoding.toLowerCase() !== 'identity') throw new Error('source fetch: encoded response is unsupported');
    const contentLength = response.headers.get('content-length');
    if (contentLength && Number(contentLength) > limits.maxBytes) throw new Error('source fetch: byte limit exceeded');
    if (!response.body) throw new Error('source fetch: missing body');
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let size = 0;
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) break;
        size += next.value.byteLength;
        if (size > limits.maxBytes) throw new Error('source fetch: byte limit exceeded');
        chunks.push(next.value);
      }
    } finally { reader.releaseLock(); }
    const bytes = Buffer.concat(chunks, size);
    const html = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    return { html, sha256: createHash('sha256').update(bytes).digest('hex') };
  } finally {
    clearTimeout(timeout);
    controller.abort();
  }
}

function evidence(value: unknown, context: ExtractionContext, path: string): asserts value is EvidenceRef {
  const ref = object(value, ['source_url', 'snapshot_sha256', 'locator'], path);
  if (ref.source_url !== context.source_url || ref.snapshot_sha256 !== context.snapshot_sha256) throw new Error(`${path}: source provenance mismatch`);
  text(ref.locator, `${path}/locator`);
}

export function validateOutcome(value: unknown, context: ExtractionContext): ExtractionOutcome {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) throw new Error('output: expected one outcome object');
  const status = (value as Record<string, unknown>).status;
  if (status === 'unsupported') {
    const result = object(value, ['status', 'reason', 'evidence'], 'output');
    if (typeof result.reason !== 'string' || !['missing_required_data', 'unsupported_format'].includes(result.reason)) throw new Error('output/reason: invalid unsupported reason');
    evidence(result.evidence, context, 'output/evidence');
  } else if (status === 'supported') {
    const result = object(value, ['status', 'venues'], 'output');
    if (!Array.isArray(result.venues) || result.venues.length < 1 || result.venues.length > 1000) throw new Error('output/venues: requires 1 to 1000 rows');
    const ids = new Set<string>();
    result.venues.forEach((row: unknown, index: number) => {
      const path = `output/venues/${index}`;
      const venue = object(row, ['venue_id', 'capacity', 'price', 'window', 'availability', 'step_free', 'evidence'], path);
      text(venue.venue_id, `${path}/venue_id`);
      if (ids.has(venue.venue_id)) throw new Error(`${path}/venue_id: duplicate`);
      ids.add(venue.venue_id);
      integer(venue.capacity, `${path}/capacity`, 0, 4294967295);
      const price = object(venue.price, ['minor_units', 'currency'], `${path}/price`);
      integer(price.minor_units, `${path}/price/minor_units`);
      if (typeof price.currency !== 'string' || !/^[A-Z]{3}$/.test(price.currency)) throw new Error(`${path}/price/currency: expected currency code`);
      window(venue.window, `${path}/window`);
      if (typeof venue.availability !== 'string' || !['available', 'unavailable'].includes(venue.availability)) throw new Error(`${path}/availability: invalid status`);
      if (typeof venue.step_free !== 'boolean') throw new Error(`${path}/step_free: expected boolean`);
      evidence(venue.evidence, context, `${path}/evidence`);
    });
  } else throw new Error('output/status: invalid status');
  const serialized = JSON.stringify(value);
  if (Buffer.byteLength(serialized, 'utf8') > MAX_OUTPUT_BYTES) throw new Error('output: byte limit exceeded');
  return JSON.parse(serialized) as ExtractionOutcome;
}

export async function execute(inputValue: unknown, extractor: Extractor, policy: SourcePolicy, limits = DEFAULT_LIMITS): Promise<ExtractionOutcome> {
  const input = validateInput(inputValue, policy);
  Object.freeze(input.requested_window);
  Object.freeze(input);
  const snapshot = await fetchSnapshot(input.source_url, limits);
  const context: ExtractionContext = Object.freeze({ source_url: input.source_url, snapshot_sha256: snapshot.sha256, input });
  return validateOutcome(await extractor(snapshot.html, context), context);
}

/** Exactly one awaited SDK push for a successful runtime invocation; no push on failure. */
export async function executeAndPublish(
  input: unknown, extractor: Extractor, policy: SourcePolicy,
  push: (outcome: ExtractionOutcome) => Promise<void>, limits = DEFAULT_LIMITS,
): Promise<void> {
  const outcome = await execute(input, extractor, policy, limits);
  await push(outcome);
}
