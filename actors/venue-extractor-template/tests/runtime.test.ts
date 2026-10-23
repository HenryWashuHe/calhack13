import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { Actor } from 'apify';
import type { ExtractionContext, ExtractionOutcome, Extractor } from '../src/contract.js';
import { extract as defaultExtract } from '../src/extractor.js';
import { createSourcePolicy, execute, executeAndPublish, validateInput, validateOutcome } from '../src/runtime.js';
import { extract as baselineExtract } from './baseline-extractor.js';

const fixtureRoot = new URL('../../../../crates/genesis-verification/examples/fixtures/portal/', import.meta.url);
const fixtures = new Map<string, Buffer>();
const requests = new Map<string, number>();
const server = createServer((request, response) => {
  const path = request.url ?? '/';
  requests.set(path, (requests.get(path) ?? 0) + 1);
  const html = (body: string | Buffer): void => { response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }); response.end(body); };
  if (path === '/redirect') { response.writeHead(302, { location: '/table.html' }); response.end(); }
  else if (path === '/too-large') html('x'.repeat(1025));
  else if (path === '/chunked') { response.writeHead(200, { 'content-type': 'text/html' }); response.write('x'.repeat(700)); response.end('y'.repeat(700)); }
  else if (path === '/slow-headers') setTimeout(() => html('<p>late</p>'), 150).unref();
  else if (path === '/slow-body') { response.writeHead(200, { 'content-type': 'text/html' }); response.write('<p>'); setTimeout(() => response.end('late</p>'), 150).unref(); }
  else if (path === '/bad-utf8') html(Buffer.from([0xc3, 0x28]));
  else if (path === '/wrong-type') { response.writeHead(200, { 'content-type': 'application/json' }); response.end('{}'); }
  else if (path === '/wrong-charset') { response.writeHead(200, { 'content-type': 'text/html; charset=iso-8859-1' }); response.end('<p>legacy</p>'); }
  else if (path === '/encoded') { response.writeHead(200, { 'content-type': 'text/html', 'content-encoding': 'gzip' }); response.end('not compressed'); }
  else if (path === '/error') { response.writeHead(503, { 'content-type': 'text/html' }); response.end('<p>unavailable</p>'); }
  else if (path === '/unknown-format') html('<main>No listings here</main>');
  else if (fixtures.has(path)) html(fixtures.get(path)!);
  else { response.writeHead(404); response.end(); }
});
let origin: string;

before(async () => {
  for (const name of ['table.html', 'cards.html', 'missing-data.html']) fixtures.set(`/${name}`, await readFile(new URL(name, fixtureRoot)));
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});
after(async () => {
  server.closeAllConnections();
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
});
function input(path: string) {
  return { source_url: origin + path, attendees: 120, requested_window: { start: '2026-11-14T18:00:00-08:00', end: '2026-11-14T22:00:00-08:00' } };
}
function policy(path: string) { return createSourcePolicy({ urls: [origin + path] }, true); }
const hash = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');

test('baseline extracts actual table bytes once, preserving unavailable rows and exact evidence', async () => {
  const beforeCount = requests.get('/table.html') ?? 0;
  const outcome = await execute(input('/table.html'), baselineExtract, policy('/table.html'));
  assert.equal(requests.get('/table.html'), beforeCount + 1);
  assert.equal(outcome.status, 'supported');
  if (outcome.status !== 'supported') assert.fail('expected table extraction');
  assert.deepEqual(outcome.venues.map((venue) => [venue.venue_id, venue.capacity, venue.price.minor_units, venue.price.currency, venue.availability, venue.step_free]), [
    ['grand-hall', 180, 75000, 'USD', 'available', true],
    ['makers-loft', 90, 25000, 'USD', 'unavailable', false],
  ]);
  assert.equal(outcome.venues[0]?.window.start, '2026-11-14T18:00:00-08:00');
  assert.equal(outcome.venues[0]?.window.end, '2026-11-14T22:00:00-08:00');
  assert.deepEqual(outcome.venues[1]?.evidence, { source_url: origin + '/table.html', snapshot_sha256: hash(fixtures.get('/table.html')!), locator: '#makers-loft' });
});

test('baseline reads the independently structured cards without hard-coded venue values', async () => {
  const outcome = await execute(input('/cards.html'), baselineExtract, policy('/cards.html'));
  assert.equal(outcome.status, 'supported');
  if (outcome.status !== 'supported') assert.fail('expected cards extraction');
  assert.deepEqual(outcome.venues.map((venue) => [venue.venue_id, venue.capacity, venue.price.minor_units, venue.price.currency]), [
    ['lakeside-room', 140, 64000, 'CAD'], ['hill-auditorium', 220, 99000, 'CAD'],
  ]);
  assert.equal(outcome.venues[1]?.window.end, '2026-11-15T18:00:00-08:00');
  assert.equal(outcome.venues[0]?.evidence.snapshot_sha256, hash(fixtures.get('/cards.html')!));
});

test('missing price is explicit unsupported output rather than an invented zero', async () => {
  const outcome = await execute(input('/missing-data.html'), baselineExtract, policy('/missing-data.html'));
  assert.deepEqual(outcome, { status: 'unsupported', reason: 'missing_required_data', evidence: { source_url: origin + '/missing-data.html', snapshot_sha256: hash(fixtures.get('/missing-data.html')!), locator: '#incomplete-gallery' } });
});

test('production default visibly refuses unsupported extraction', async () => {
  const outcome = await execute(input('/table.html'), defaultExtract, policy('/table.html'));
  assert.equal(outcome.status, 'unsupported');
  if (outcome.status === 'unsupported') assert.equal(outcome.reason, 'unsupported_format');
});

test('source policy is immutable, deny-by-default, and cannot be supplied in Actor input', async () => {
  const builtPolicy = policy('/table.html');
  assert.ok(Object.isFrozen(builtPolicy));
  assert.ok(Object.isFrozen(builtPolicy.urls));
  assert.throws(() => createSourcePolicy({ urls: [origin + '/table.html'] }), /HTTPS/);
  assert.throws(() => createSourcePolicy({ urls: ['https://example.com/table#fragment'] }), /HTTPS/);
  assert.throws(() => createSourcePolicy({ urls: ['https://user:password@example.com/table'] }), /HTTPS/);
  assert.throws(() => createSourcePolicy({ urls: ['https://example.com/table', 'https://example.com/table'] }), /duplicate/);
  await assert.rejects(execute(input('/table.html'), baselineExtract, createSourcePolicy({ urls: [] })), /allowlist/);
  await assert.rejects(execute({ ...input('/table.html'), allowed_sources: [origin] }, baselineExtract, builtPolicy), /incorrect fields/);
  assert.throws(() => validateInput({ ...input('/table.html'), source_url: origin + '/cards.html' }, builtPolicy), /allowlist/);
});

test('input validation rejects missing offsets, impossible dates, reversed windows and unsafe counts', () => {
  for (const start of ['2026-11-14T18:00:00', '2026-02-30T18:00:00Z', '2026-11-15T18:00:00-08:00']) {
    assert.throws(() => validateInput({ ...input('/table.html'), requested_window: { start, end: '2026-11-14T22:00:00-08:00' } }, policy('/table.html')));
  }
  for (const attendees of [0, -1, 1.5, Number.MAX_SAFE_INTEGER, '120']) assert.throws(() => validateInput({ ...input('/table.html'), attendees }, policy('/table.html')));
});

test('redirects are rejected without visiting their target', async () => {
  const count = requests.get('/table.html') ?? 0;
  await assert.rejects(execute(input('/redirect'), baselineExtract, policy('/redirect')));
  assert.equal(requests.get('/table.html') ?? 0, count);
});

test('fetch bounds cover declared length, chunked streams and slow headers/body', async () => {
  for (const path of ['/too-large', '/chunked']) await assert.rejects(execute(input(path), baselineExtract, policy(path), { maxBytes: 1024, timeoutMs: 1000 }), /byte limit/);
  for (const path of ['/slow-headers', '/slow-body']) await assert.rejects(execute(input(path), baselineExtract, policy(path), { maxBytes: 1024, timeoutMs: 30 }));
});

test('bad responses do not reach the extractor or publish data', async () => {
  for (const path of ['/bad-utf8', '/wrong-type', '/wrong-charset', '/encoded', '/error']) {
    let calls = 0;
    let pushes = 0;
    const extractor: Extractor = (_html, context) => { calls++; return defaultExtract('', context); };
    await assert.rejects(executeAndPublish(input(path), extractor, policy(path), async () => { pushes++; }));
    assert.equal(calls, 0, path);
    assert.equal(pushes, 0, path);
  }
});

test('runtime freezes context and rejects forged provenance before publication', async () => {
  const extractor: Extractor = (_html, context) => {
    assert.ok(Object.isFrozen(context));
    assert.ok(Object.isFrozen(context.input));
    assert.ok(Object.isFrozen(context.input.requested_window));
    const outcome = defaultExtract('', context);
    if (outcome.status === 'unsupported') outcome.evidence.snapshot_sha256 = '0'.repeat(64);
    return outcome;
  };
  let pushed = false;
  await assert.rejects(executeAndPublish(input('/table.html'), extractor, policy('/table.html'), async () => { pushed = true; }), /provenance/);
  assert.equal(pushed, false);
});

test('one awaited outcome is published and publication errors propagate', async () => {
  const outputs: ExtractionOutcome[] = [];
  let finishedPush = false;
  await executeAndPublish(input('/cards.html'), baselineExtract, policy('/cards.html'), async (outcome) => {
    await Promise.resolve(); outputs.push(outcome); finishedPush = true;
  });
  assert.equal(outputs.length, 1);
  assert.equal(finishedPush, true);
  await assert.rejects(executeAndPublish(input('/cards.html'), baselineExtract, policy('/cards.html'), async () => { throw new Error('dataset write failed'); }), /dataset write failed/);
});

test('real Apify SDK reads local input and persists exactly one dataset outcome', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'genesis-actor-sdk-'));
  const actor = new Actor({ persistStorage: true, purgeOnStart: false, logLevel: 0 });
  // ApifyClient and local MemoryStorage interpret storageClientOptions differently.
  // Set MemoryStorage's directory after the Actor constructs its API client.
  actor.config.set('storageClientOptions', { localDataDirectory: directory });
  // Force the SDK's local branch; no provider request is part of this test.
  actor.isAtHome = () => false;
  actor.config.set('isAtHome', false);
  actor.config.set('apiBaseUrl', origin);
  actor.config.set('inputKey', 'INPUT');
  actor.config.set('defaultDatasetId', 'local-test-dataset');
  actor.config.set('defaultKeyValueStoreId', 'local-test-input');
  try {
    await actor.init({ gracefulShutdown: false });
    const store = await actor.openKeyValueStore();
    await store.setValue('INPUT', input('/table.html'));
    const supplied = await actor.getInput<unknown>();
    await executeAndPublish(supplied, baselineExtract, policy('/table.html'), async (outcome) => { await actor.pushData(outcome); });
    const dataset = await actor.openDataset();
    const data = await dataset.getData();
    assert.equal(data.count, 1);
    assert.equal(data.items.length, 1);
    assert.equal(data.items[0]?.status, 'supported');
    const item = data.items[0] as unknown as ExtractionOutcome;
    if (item.status !== 'supported') assert.fail();
    assert.equal(item.venues[0]?.evidence.snapshot_sha256, hash(fixtures.get('/table.html')!));
    const persisted = JSON.parse(await readFile(join(directory, 'datasets', 'local-test-dataset', '000000001.json'), 'utf8')) as ExtractionOutcome;
    assert.deepEqual(persisted, item);
  } finally {
    await actor.exit({ exit: false, timeoutSecs: 1 });
    await rm(directory, { recursive: true, force: true });
  }
});

test('schema validation rejects duplicate rows, unknown fields, unsafe prices and malformed outcomes', () => {
  const context: ExtractionContext = { source_url: 'https://example.com/table', snapshot_sha256: 'a'.repeat(64), input: { source_url: 'https://example.com/table', attendees: 120, requested_window: { start: '2026-11-14T18:00:00-08:00', end: '2026-11-14T22:00:00-08:00' } } };
  const valid = baselineExtract(fixtures.get('/table.html')!.toString('utf8'), context);
  assert.equal(valid.status, 'supported');
  if (valid.status !== 'supported') assert.fail();
  const duplicates = structuredClone(valid); duplicates.venues.push(duplicates.venues[0]!);
  assert.throws(() => validateOutcome(duplicates, context), /duplicate/);
  const unsafe = structuredClone(valid); unsafe.venues[0]!.price.minor_units = Number.MAX_SAFE_INTEGER + 1;
  assert.throws(() => validateOutcome(unsafe, context), /integer/);
  for (const malformed of [null, [], { status: 'supported', venues: [] }, { ...valid, approved: true }]) assert.throws(() => validateOutcome(malformed, context));
});
