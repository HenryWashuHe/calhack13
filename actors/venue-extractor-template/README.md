# GENESIS venue Actor scaffold

This is a runnable TypeScript Actor runtime for a narrowly scoped web extraction capability. The forge replaces `src/extractor.ts` and writes the permitted exact URLs to `allowed-sources.json` before hashing and building the source package. The default extractor returns `unsupported_format`; it never supplies pretend venue results.

## Build and test

Requires Node.js 24 or later. From this directory:

```sh
npm ci --include=dev --ignore-scripts
npm test
```

Tests compile the production and test TypeScript, run a real loopback HTTP fixture server, and exercise the actual Apify SDK with temporary local storage. They cover both supported HTML layouts, missing data, exact source hashes, unavailable venues, redirects, chunked and declared response limits, header/body timeouts, malformed UTF-8, input/output validation, and publication failures. Fixture HTML comes from the public portal files under `crates/genesis-verification/examples/fixtures/portal`; these tests never read the trusted oracle.

The initial implementation was compiled and tested on Node.js 25.4.0. The Dockerfile targets `apify/actor-node:24-slim`. Docker execution and live Apify runs require separate integration validation; local SDK storage is not evidence of a cloud execution.

## Generated module contract

```ts
import type { ExtractionContext, ExtractionOutcome } from './contract.js';

export function extract(
  html: string,
  context: ExtractionContext,
): ExtractionOutcome | Promise<ExtractionOutcome> {
  // Parse only facts present in the supplied HTML.
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
```

`src/contract.ts` matches the verification crate's JSON outcome contract. Supported output contains every extracted venue, including unavailable and undersized venues; mission filtering belongs elsewhere. Prices use nonnegative integer minor units and uppercase currency codes. JavaScript output is limited to safe integers so it cannot silently round Rust `u64` price values. Time windows require explicit offsets and positive duration. `step_free` describes a source-declared attribute, not an accessibility audit.

The parser receives immutable source provenance and the validated Actor input:

```json
{
  "source_url": "https://fixtures.example/venues/table.html",
  "attendees": 120,
  "requested_window": {
    "start": "2026-11-14T18:00:00-08:00",
    "end": "2026-11-14T22:00:00-08:00"
  }
}
```

The input cannot change the URL policy, byte limit, timeout, or provenance. The exact URL must appear in the build-time file:

```json
{ "urls": ["https://fixtures.example/venues/table.html"] }
```

An empty list denies all fetches. The production entrypoint accepts canonical HTTPS URLs without credentials or fragments. The test API has an explicit loopback HTTP option; `src/main.ts` never enables it.

## Runtime behavior and boundaries

The runtime performs one source fetch, forbids redirects, caps the body at 2 MiB, and applies a 10-second deadline across headers and body. It requires HTML with UTF-8 encoding, requests identity content encoding, and rejects encoded responses. The SHA-256 covers the exact received body bytes before UTF-8 decoding. The parser is called only after the complete response has passed these checks.

The runtime validates the outcome and rejects missing fields, unknown fields, duplicate venue IDs, unsafe numbers, invalid windows, and evidence that contradicts the fetched URL or source digest. This is structural and provenance validation; semantic correctness still requires GENESIS's independent oracle suite.

For a successful invocation, `executeAndPublish` makes one awaited `Actor.pushData` call with one outcome object. Fetch, parser, validation, and dataset errors fail the run. Publication retries or rerunning an Actor are not globally exactly-once; the trusted forge adapter must check dataset cardinality and bind the actual run/build/dataset references to its attempt.

The allowlist and frozen context are runtime guardrails, **not an arbitrary-code sandbox**. Generated TypeScript runs in the same Node process and can import filesystem, network, process, or SDK APIs. These checks do not establish egress isolation, stop malicious code from bypassing the runtime, or prevent an infinite synchronous parser loop. Use only public synthetic data for the demo; provider runtime limits and stronger process isolation remain separate responsibilities. No oracle answers, credentials, or verification policy should enter the generated module or build payload.

## Hand-authored baseline

`tests/baseline-extractor.ts` is a hand-authored parser used to test the runtime against public table/card markup. It is **not AI-generated** and is excluded from production packages. It reads prices, capacities, IDs, times, availability, and declared access from HTML without embedding fixture answers. Missing numeric prices produce `missing_required_data`.

To use that baseline for a labeled local forge integration test, copy its source as the candidate module and change its type-only import from `../src/contract.js` to `./contract.js`. A live generation demonstration must supply newly generated source instead.

## Source package boundary

The forge packages only these production files:

```text
.actor/actor.json
.actor/input_schema.json
Dockerfile
package.json
package-lock.json
tsconfig.json
allowed-sources.json
src/contract.ts
src/runtime.ts
src/main.ts
src/extractor.ts
```

Tests, the baseline, README, portal fixtures, oracle data, local storage, `node_modules`, and compiled output are excluded. The package lock pins npm dependency resolutions. The Docker tag pins the Node major version but remains mutable; a source digest alone does not attest the final container image. The forge must retain the actual provider build ID/number and independently verify that build.

The lockfile overrides `get-uri`'s transitive `basic-ftp` dependency to 6.2.3
because its declared 5.x range includes the known
[directory-listing denial of service](https://github.com/advisories/GHSA-c475-qrg2-pj4r).
The 6.x API retains the methods used by `get-uri`; its intentional behavior change
denies separate FTP transfer hosts by default. This Actor fetches only HTTPS,
but the SDK's optional proxy dependency still brings that package into the image.
The override is exercised by the SDK local-storage checks and the cross-language
demo; no FTP support is claimed by GENESIS.

The runtime uses the current official [`Actor.getInput`, `Actor.main`, and `Actor.pushData` APIs](https://docs.apify.com/sdk/js/reference/class/Actor). Actor metadata and the input schema follow [Apify's Actor definition format](https://docs.apify.com/actors/development/actor-definition/actor-json). The [official Docker guidance](https://docs.apify.com/sdk/js/docs/concepts/docker-images) recommends a versioned slim base with dependencies declared in `package.json`; the build stage explicitly includes development dependencies despite the base image's production environment.
