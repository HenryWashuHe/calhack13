// Independent offline regressions. All HTTP is injected; only synthetic keys are used.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { runTrigger } from './creao-trigger.mjs';

const APP = '00000000-0000-4000-8000-000000000001';
const RUN = '11111111-1111-4111-8111-111111111111';
const KEY = 'capi_independent-offline-fixture';
const response = (status, extra = {}, http = 200) => new Response(JSON.stringify({
  id: RUN, appId: APP, status, ...extra,
}), { status: http });
function fixture(t) {
  const dir = fs.mkdtempSync(join(tmpdir(), 'creao-trigger-review-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return { appId: APP, inputs: { task: 'offline fixture' }, out: join(dir, 'run.json'),
    allowCredits: true, key: KEY, maxPolls: 1, pollMs: 0 };
}

test('pre-POST intent syncs its parent directory before any request', async (t) => {
  const config = fixture(t);
  const original = fs.fsyncSync;
  const synced = [];
  let atPost;
  fs.fsyncSync = (fd) => {
    synced.push(fs.fstatSync(fd).isDirectory() ? 'directory' : 'file');
    return original(fd);
  };
  syncBuiltinESMExports();
  try {
    await runTrigger({ ...config, fetchImpl: async (_url, options) => {
      assert.equal(options.method, 'POST');
      atPost = [...synced];
      throw new Error('offline uncertain submission');
    } });
  } finally {
    fs.fsyncSync = original;
    syncBuiltinESMExports();
  }
  assert.ok(atPost.includes('file'), 'intent content must be synced');
  assert.ok(atPost.includes('directory'), 'new journal name must survive before POST');
});

test('streamed response limit stops consuming and cancels the oversized body', async (t) => {
  const config = fixture(t);
  let chunks = 0;
  let cancelled = false;
  const stream = new ReadableStream({
    pull(controller) {
      if (chunks === 20) { controller.close(); return; }
      chunks++;
      controller.enqueue(new Uint8Array(64 * 1024).fill(32));
    },
    cancel() { cancelled = true; },
  });
  const report = await runTrigger({ ...config,
    fetchImpl: async () => new Response(stream, { status: 202 }) });
  assert.equal(report.phase, 'needs_reconciliation');
  // Allow one stream-prefetched chunk beyond the chunk detecting the 256 KiB cap.
  assert.ok(chunks <= 6, `read ${chunks} chunks before enforcing the response cap`);
  assert.equal(cancelled, true);
});

test('a delayed concurrent pending poll cannot erase completed evidence', async (t) => {
  const config = fixture(t);
  await runTrigger({ ...config, fetchImpl: async (_url, options) => {
    if (options.method === 'POST') return response('pending', {}, 202);
    throw new Error('offline initial GET interruption');
  } });
  let finishDelayed;
  let started;
  const didStart = new Promise((resolve) => { started = resolve; });
  const delayed = runTrigger({ ...config, fetchImpl: async (_url, options) => {
    assert.equal(options.method, 'GET');
    started();
    return new Promise((resolve) => { finishDelayed = resolve; });
  } });
  await didStart;
  const second = runTrigger({ ...config, fetchImpl: async (_url, options) => {
    assert.equal(options.method, 'GET');
    return response('completed', { result: { answer: 'fixture' }, usage: { credits: 2 } });
  } }).then((value) => ({ value }), (error) => ({ error }));
  // Let an unguarded second poll finish. A correct lock may reject it or queue
  // it until the first completes, both of which must also be safe.
  await new Promise(setImmediate);
  finishDelayed(response('pending'));
  await delayed;
  const outcome = await second;
  if (outcome.error) {
    assert.match(outcome.error.message, /lock|busy|progress|journal/i);
    return;
  }
  const completed = outcome.value;
  assert.equal(completed.phase, 'completed');
  const retained = JSON.parse(fs.readFileSync(config.out, 'utf8'));
  assert.equal(retained.phase, 'completed', 'stale poll must not overwrite a terminal record');
  assert.equal(retained.credits_used, 2);
  assert.equal(retained.result_sha256, completed.result_sha256);
});
