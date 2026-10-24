// PR #60 review regression. Run from a clean checkout with Node:
//   node --test scripts/venue-portal/review-holdout-index.test.mjs
// Uses only a temporary portal and a loopback HTTP server; no provider/tunnel.
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { presetSpec, sha256, withHoldoutExamples } from "./lib.mjs";
import { writeVariant } from "./generate.mjs";
import { publicHandler } from "./serve.mjs";

test("every held-out portal index link serves the page whose digest it displays", async () => {
  const out = mkdtempSync(join(tmpdir(), "portal-index-review-"));
  const result = writeVariant({
    spec: withHoldoutExamples(presetSpec("baseline", "review"), "review"),
    baseUrl: "https://portal.example",
    out,
    namespace: "org-a",
  });
  const server = createServer(publicHandler(join(out, "public")));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const indexUrl = `http://127.0.0.1:${server.address().port}/org-a/`;
    const indexResponse = await fetch(indexUrl);
    assert.equal(indexResponse.status, 200);
    const html = await indexResponse.text();
    const links = [...html.matchAll(/href="([^"]+)"[^>]*>[^<]*<\/a> <code>([^<]+)/g)];
    assert.equal(links.length, result.pages.length);
    for (const [, href, advertisedDigest] of links) {
      const page = result.pages.find((p) => p.sha256.startsWith(advertisedDigest));
      assert.ok(page, "each displayed digest belongs to an emitted page");
      const response = await fetch(new URL(href, indexUrl));
      assert.equal(response.status, 200);
      const servedDigest = sha256(Buffer.from(await response.arrayBuffer()));
      assert.equal(servedDigest, page.sha256, `${page.role} ${page.path} links to the wrong page: ${href}`);
    }
  } finally {
    await new Promise((resolve) => server.close(resolve));
    rmSync(out, { recursive: true, force: true });
  }
});
