#!/usr/bin/env node
// Offline integration demo, not AI synthesis or authenticated cloud execution.
// Uses only Node built-ins until npm installs the exact forged package lockfile.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const help = `Usage: node scripts/demo-local-capability.mjs [--output-dir NEW_DIRECTORY]

Requires Node >=24, npm, and Rust >=1.94/cargo on PATH. npm ci and cargo may
download locked dependencies. Actor execution uses loopback HTTP and actual
Apify SDK local storage; no cloud Actor/build/run is created. The parser is a
hand-authored baseline, not AI synthesis. Oracle answers stay in the evaluator.
Temporary files are removed by default; --output-dir preserves inspectable
artifacts in a new directory. Any failed assertion exits nonzero.
`;

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
// GENESIS v1 digest: sorted object keys, preserved array order. This demo uses
// ASCII keys and safe integers only; it is not a general RFC 8785 implementation.
function jsonSha256(value) {
  const sorted = (item) => Array.isArray(item) ? item.map(sorted)
    : item !== null && typeof item === 'object'
      ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, sorted(item[key])])) : item;
  return sha256(JSON.stringify(sorted(value)));
}
const readJson = async (path) => JSON.parse(await readFile(path, 'utf8'));
const writeJson = async (path, value) => writeFile(path, `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
const progress = (message) => process.stderr.write(`[local capability demo] ${message}\n`);

async function command(program, args, options = {}) {
  const { cwd = repo, env = process.env, timeout = 180_000, expectedCode = 0 } = options;
  const result = await new Promise((resolveResult, reject) => {
    // npm/cargo can leave grandchildren holding inherited stdout/stderr open.
    // Give the whole command a process group so timeout cleanup reaches them.
    const grouped = process.platform !== 'win32';
    const child = spawn(program, args, { cwd, env, detached: grouped, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let settled = false;
    let closeFallback;
    const settle = (code, signal, error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(closeFallback);
      if (error) reject(error);
      else resolveResult({ code, signal, timedOut, stdout, stderr });
    };
    const timer = setTimeout(() => {
      timedOut = true;
      let groupKilled = false;
      if (grouped && child.pid) {
        try { process.kill(-child.pid, 'SIGKILL'); groupKilled = true; } catch { /* Fall back to the immediate child. */ }
      }
      if (!groupKilled) { try { child.kill('SIGKILL'); } catch { /* The bounded close fallback still settles. */ } }
      // A descendant may escape the group or a signal may be denied. Stop
      // waiting on inherited pipes after a bounded drain interval either way.
      closeFallback = setTimeout(() => {
        child.stdout.destroy();
        child.stderr.destroy();
        child.unref();
        settle(null, 'SIGKILL');
      }, 1_000);
    }, timeout);
    const collect = (previous, chunk) => (previous + chunk.toString()).slice(-2 * 1024 * 1024);
    child.stdout.on('data', (chunk) => { stdout = collect(stdout, chunk); });
    child.stderr.on('data', (chunk) => { stderr = collect(stderr, chunk); });
    child.once('error', (error) => { settle(null, null, error); });
    child.once('close', (code, signal) => { settle(code, signal); });
  });
  assert.equal(result.timedOut, false, `${program} exceeded ${timeout}ms`);
  assert.equal(result.code, expectedCode,
    `${program} exited ${result.code ?? result.signal}, expected ${expectedCode}\n${result.stderr}\n${result.stdout}`);
  return result.stdout;
}

// This child receives one input, a source allowlist, and local storage settings.
// It receives no suite, expected answers, receipts, or evaluator artifact pin.
// The baseline is trusted demo code: process separation is NOT a code sandbox.
const workerSource = String.raw`
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
const [packageDir, jobPath, resultPath] = process.argv.slice(2);
const job = JSON.parse(await readFile(jobPath, 'utf8'));
assert.deepEqual(Object.keys(job).sort(), ['dataset_id', 'input', 'origin', 'storage_dir']);
const requirePackage = createRequire(join(packageDir, 'package.json'));
const { Actor } = requirePackage('apify');
const { executeAndPublish, createSourcePolicy } = await import(pathToFileURL(join(packageDir, 'dist/runtime.js')));
const { extract } = await import(pathToFileURL(join(packageDir, 'dist/extractor.js')));
const bakedPolicy = JSON.parse(await readFile(join(packageDir, 'allowed-sources.json'), 'utf8'));
const urls = bakedPolicy.urls.map((url) => {
  const source = new URL(url);
  assert.equal(source.origin, 'https://fixtures.genesis.example');
  return job.origin + source.pathname;
});
// Explicit test-only transport override; production main.ts stays untouched.
const policy = createSourcePolicy({ urls }, true);
const actor = new Actor({ persistStorage: true, purgeOnStart: false, logLevel: 0 });
actor.config.set('storageClientOptions', { localDataDirectory: job.storage_dir });
actor.isAtHome = () => false;
actor.config.set('isAtHome', false);
actor.config.set('apiBaseUrl', job.origin);
actor.config.set('inputKey', 'INPUT');
actor.config.set('defaultDatasetId', job.dataset_id);
actor.config.set('defaultKeyValueStoreId', 'local-synthetic-input');
try {
  await actor.init({ gracefulShutdown: false });
  const store = await actor.openKeyValueStore();
  await store.setValue('INPUT', job.input);
  const actualInput = await actor.getInput();
  assert.deepEqual(actualInput, job.input);
  let pushes = 0;
  await executeAndPublish(actualInput, extract, policy, async (outcome) => {
    pushes++;
    await actor.pushData(outcome);
  });
  assert.equal(pushes, 1);
  const dataset = await actor.openDataset();
  const page = await dataset.getData();
  assert.equal(page.count, 1);
  assert.equal(page.total, 1);
  assert.equal(page.items.length, 1);
  const persisted = JSON.parse(await readFile(join(job.storage_dir, 'datasets', job.dataset_id, '000000001.json'), 'utf8'));
  assert.deepEqual(persisted, page.items[0]);
  await writeFile(resultPath, JSON.stringify({ input: actualInput, dataset_id: job.dataset_id, output: persisted }), { flag: 'wx', mode: 0o600 });
} finally {
  await actor.exit({ exit: false, timeoutSecs: 1 });
}
`;

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && ['--help', '-h'].includes(args[0])) { process.stdout.write(help); return; }
  assert.ok(args.length === 0 || (args.length === 2 && args[0] === '--output-dir' && !args[1].startsWith('--')), help);
  assert.ok(Number(process.versions.node.split('.')[0]) >= 24, 'Node >=24 is required by the packaged Actor');
  const keep = args.length !== 0;
  const directory = keep ? resolve(args[1]) : await mkdtemp(join(tmpdir(), 'genesis-local-capability-'));
  if (keep) await mkdir(directory, { mode: 0o700 }); // Refuse an existing output directory.
  let server;
  try {
    progress('Building Rust forge/verifier; dependency downloads may use the network.');
    // Explicit target-dir makes paths deterministic even with CARGO_TARGET_DIR set.
    const rustTarget = join(repo, 'crates/genesis-forge/target');
    await command('cargo', ['build', '--locked', '--manifest-path', 'crates/genesis-forge/Cargo.toml', '--target-dir', rustTarget, '--bin', 'genesis-forge'], { timeout: 600_000 });
    await command('cargo', ['build', '--locked', '--manifest-path', 'crates/genesis-verification/Cargo.toml', '--target-dir', rustTarget, '--bin', 'genesis-verify'], { timeout: 600_000 });
    const forge = join(rustTarget, 'debug/genesis-forge');
    const verifier = join(rustTarget, 'debug/genesis-verify');
    const evaluator = join(directory, 'evaluator');
    const packageDir = join(directory, 'package');
    const workers = join(directory, 'workers');
    await mkdir(evaluator);
    await mkdir(packageDir);
    await mkdir(workers);
    const trusted = await readJson(join(repo, 'crates/genesis-verification/examples/fixtures/trusted/suite.json'));
    const trustedPath = join(evaluator, 'trusted-original-suite.json');
    await writeJson(trustedPath, trusted);
    const metadata = {
      operation_id: 'local-baseline-forge', capability_id: 'venue-availability', capability_version: '0.1.0',
      actor_name: 'local-baseline-venue-extractor', source_version: '0.1',
    };
    const metadataPath = join(evaluator, 'candidate.json');
    await writeJson(metadataPath, metadata);
    const baseline = await readFile(join(repo, 'actors/venue-extractor-template/tests/baseline-extractor.ts'), 'utf8');
    assert.ok(baseline.includes("from '../src/contract.js'"), 'baseline import layout changed');
    const baselinePath = join(evaluator, 'baseline.ts');
    // Only adapt the type import to the packaged src/extractor.ts location.
    await writeFile(baselinePath, baseline.replace("from '../src/contract.js'", "from './contract.js'"), { flag: 'wx', mode: 0o600 });
    const checkpointPath = join(evaluator, 'prepared-checkpoint.json');
    progress('Preparing the hand-authored baseline through the actual forge CLI (offline).');
    const prepare = JSON.parse(await command(forge, ['prepare', '--candidate', metadataPath, '--extractor', baselinePath, '--suite', trustedPath, '--checkpoint', checkpointPath]));
    assert.equal(prepare.phase, 'prepared');
    const checkpoint = await readJson(checkpointPath);
    const files = checkpoint.spec.source.files;
    assert.ok(Array.isArray(files) && files.length > 0);
    const seen = new Set();
    for (const file of files) {
      assert.match(file.name, /^[A-Za-z0-9._/-]+$/);
      assert.ok(file.name.split('/').every((part) => part && part !== '.' && part !== '..'));
      assert.ok(!seen.has(file.name));
      seen.add(file.name);
      assert.equal(typeof file.content, 'string');
      await mkdir(dirname(join(packageDir, file.name)), { recursive: true });
      await writeFile(join(packageDir, file.name), file.content, { flag: 'wx' });
    }
    assert.deepEqual([...seen].sort(), ['.actor/actor.json', '.actor/input_schema.json', 'Dockerfile', 'allowed-sources.json', 'package-lock.json', 'package.json', 'src/contract.ts', 'src/extractor.ts', 'src/main.ts', 'src/runtime.ts', 'tsconfig.json'].sort());
    assert.ok(files.every((file) => !file.content.includes('offline-venue-suite-v1')), 'oracle suite accidentally packaged');
    const manifest = { format: 'genesis-source-v1', files: files.map((file) => ({ path: file.name, byte_length: Buffer.byteLength(file.content), sha256: sha256(file.content) })).sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0) };
    const sourceSha256 = jsonSha256(manifest);
    // Compare against Rust's fingerprint, which includes its own package digest.
    assert.equal(checkpoint.request_sha256, jsonSha256({ format: 'genesis-candidate-v1', ...metadata, source_sha256: sourceSha256 }));
    await writeJson(join(evaluator, 'source-manifest.json'), manifest);
    progress('Installing the forged lockfile in the fresh package and compiling its TypeScript once.');
    await command('npm', ['ci', '--include=dev', '--ignore-scripts', '--no-audit', '--no-fund'], { cwd: packageDir, timeout: 600_000 });
    await command('npm', ['run', 'build'], { cwd: packageDir });

    const fixtures = new Map();
    for (const name of ['table.html', 'cards.html', 'missing-data.html']) {
      fixtures.set(`/venues/${name}`, await readFile(join(repo, 'crates/genesis-verification/examples/fixtures/portal', name)));
    }
    const requests = [];
    server = createServer((request, response) => {
      const bytes = fixtures.get(request.url);
      requests.push({ method: request.method, path: request.url, fixture: !!bytes });
      if (request.method !== 'GET' || !bytes) { response.writeHead(404); response.end(); return; }
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-length': bytes.length });
      response.end(bytes);
    });
    await new Promise((resolveListen, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolveListen); });
    const origin = `http://127.0.0.1:${server.address().port}`;
    const workerPath = join(workers, 'run-local-actor.mjs');
    await writeFile(workerPath, workerSource, { flag: 'wx' });
    const localUrl = (url) => {
      const parsed = new URL(url);
      assert.equal(parsed.origin, 'https://fixtures.genesis.example');
      assert.ok(fixtures.has(parsed.pathname));
      return origin + parsed.pathname;
    };
    function suiteFor(org) {
      // Transform transport only; never derive expected facts from Actor output.
      const suite = structuredClone(trusted);
      suite.suite_id = `local-baseline-${org}-v1`;
      suite.scope = 'Local hand-authored baseline: loopback fixtures and real SDK storage. Synthetic provider IDs; no cloud attestation, publication, or booking authority.';
      for (const test of suite.cases) {
        assert.equal(sha256(fixtures.get(new URL(test.source.url).pathname)), test.source.sha256);
        test.input.source_url = localUrl(test.input.source_url);
        test.source.url = localUrl(test.source.url);
        if (org === 'org-b') test.input.attendees += 1;
        const refs = test.expected.status === 'supported' ? test.expected.venues.map((venue) => venue.evidence) : [test.expected.evidence];
        for (const ref of refs) ref.source_url = localUrl(ref.source_url);
      }
      return suite;
    }
    const pin = {
      capability_id: metadata.capability_id, capability_version: metadata.capability_version,
      source_sha256: sourceSha256, actor_id: 'local-synthetic-actor', build_id: 'local-synthetic-build', build_number: '0.1.1',
    };
    const pinPath = join(evaluator, 'local-synthetic-artifact.json');
    await writeJson(pinPath, pin);
    // Do not inherit provider credentials or APIFY_* configuration into workers.
    const workerEnv = { PATH: process.env.PATH ?? '', TMPDIR: tmpdir(), NO_COLOR: '1' };
    async function runOrganization(org, suite) {
      const receipts = [];
      for (const test of suite.cases) {
        const stem = `${org}-${test.case_id}`;
        const datasetId = `local-synthetic-${stem}`;
        const jobPath = join(workers, `${stem}-input.json`);
        const resultPath = join(workers, `${stem}-result.json`);
        await writeJson(jobPath, { input: test.input, dataset_id: datasetId, origin, storage_dir: join(workers, `${stem}-storage`) });
        await command(process.execPath, [workerPath, packageDir, jobPath, resultPath], { cwd: packageDir, env: workerEnv, timeout: 60_000 });
        const result = await readJson(resultPath);
        assert.deepEqual(result.input, test.input);
        assert.equal(result.dataset_id, datasetId);
        receipts.push({
          case_id: test.case_id, artifact: pin, input_sha256: jsonSha256(result.input),
          source: { url: result.input.source_url, sha256: sha256(fixtures.get(new URL(result.input.source_url).pathname)) },
          run_id: `local-synthetic-run-${stem}`, dataset_id: datasetId, status: 'succeeded', output: result.output,
        });
      }
      return receipts;
    }
    async function verify(label, suite, receipts, expectedCode) {
      const suitePath = join(evaluator, `${label}-suite.json`);
      const receiptsPath = join(evaluator, `${label}-receipts.json`);
      await writeJson(suitePath, suite);
      await writeJson(receiptsPath, receipts);
      const report = JSON.parse(await command(verifier, ['--suite', suitePath, '--artifact', pinPath, '--runs', receiptsPath], { expectedCode }));
      assert.equal(report.decision, expectedCode === 0 ? 'accepted' : 'rejected');
      assert.deepEqual(report.artifact, pin);
      await writeJson(join(evaluator, `${label}-report.json`), report);
      return report;
    }
    progress('Org A: running all public fixtures through the packaged runtime and real SDK storage.');
    const suiteA = suiteFor('org-a');
    const receiptsA = await runOrganization('org-a', suiteA);
    const reportA = await verify('org-a-accepted', suiteA, receiptsA, 0);
    const changed = structuredClone(receiptsA);
    const supported = changed.find((receipt) => receipt.output.status === 'supported');
    assert.ok(supported?.output.venues.length);
    supported.output.venues[0].capacity += 1;
    const rejection = await verify('one-fact-rejected', suiteA, changed, 1);
    assert.ok(rejection.cases.some((test) => test.issues.some((issue) => issue.code === 'value_mismatch' && issue.path.endsWith('/capacity'))));
    progress('Org A accepted; changing one capacity fact rejected. Org B now reuses the same compiled package with its own input.');
    const suiteB = suiteFor('org-b');
    const receiptsB = await runOrganization('org-b', suiteB);
    const reportB = await verify('org-b-accepted', suiteB, receiptsB, 0);
    const replay = await verify('org-a-receipts-rejected-for-b', suiteB, receiptsA, 1);
    assert.ok(replay.cases.every((test) => test.issues.some((issue) => issue.code === 'input_mismatch')));
    assert.notEqual(reportA.suite_sha256, reportB.suite_sha256);
    for (let index = 0; index < receiptsA.length; index++) {
      assert.notEqual(receiptsA[index].input_sha256, receiptsB[index].input_sha256);
      assert.notEqual(receiptsA[index].dataset_id, receiptsB[index].dataset_id);
      assert.deepEqual(receiptsA[index].artifact, receiptsB[index].artifact);
    }
    assert.equal(requests.length, trusted.cases.length * 2);
    assert.ok(requests.every((request) => request.method === 'GET' && request.fixture), 'unexpected local API request');
    for (const file of files) assert.equal(await readFile(join(packageDir, file.name), 'utf8'), file.content, `packaged source changed: ${file.name}`);
    const summary = {
      demo: 'local hand-authored baseline, not AI synthesis',
      execution: 'actual packaged runtime and Apify SDK local storage; test-only loopback transport override',
      provenance: 'all Actor/build/run IDs are synthetic local labels; no authenticated cloud execution',
      authority: 'none: acceptance does not publish, install, or authorize booking',
      source_sha256: sourceSha256, package_compilations: 1, sdk_local_runs: receiptsA.length + receiptsB.length,
      org_a: reportA.decision, changed_capacity: rejection.decision, org_b: reportB.decision,
      org_a_receipts_for_b: replay.decision, org_b_new_source_packages: 0,
      preserved_output: keep ? directory : null,
    };
    await writeJson(join(directory, 'summary.json'), summary);
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  } finally {
    if (server?.listening) {
      server.closeAllConnections();
      await new Promise((resolveClose, reject) => server.close((error) => error ? reject(error) : resolveClose()));
    }
    if (!keep) await rm(directory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  process.stderr.write(`local capability demo failed: ${error.stack ?? error}\n`);
  process.exitCode = 1;
});
