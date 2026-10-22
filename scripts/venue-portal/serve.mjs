#!/usr/bin/env node
// Serve the synthetic venue portal.
//
//   node scripts/venue-portal/serve.mjs --out data/portal [--port 7200] [--lab-port 7201]
//
// Port 7200 serves only <out>/public (point a tunnel here). The lab on 7201
// binds 127.0.0.1 only: a judge picks a disruption and the variant is
// regenerated in place. Neither server ever reads <out>/trusted.
import { createServer } from "node:http";
import { readFileSync, statSync } from "node:fs";
import { extname, join, resolve, sep } from "node:path";
import { parseArgs } from "node:util";
import { presetSpec, sha256 } from "./lib.mjs";
import { writeVariant } from "./generate.mjs";

const TYPES = { ".html": "text/html; charset=utf-8", ".json": "application/json", ".txt": "text/plain; charset=utf-8" };

export function publicHandler(publicDir) {
  const root = resolve(publicDir);
  return (req, res) => {
    if (req.method !== "GET" && req.method !== "HEAD") {
      res.writeHead(405, { allow: "GET, HEAD" }).end();
      return;
    }
    let path;
    try {
      path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    } catch {
      res.writeHead(400).end();
      return;
    }
    if (path.endsWith("/")) path += "index.html";
    const file = resolve(join(root, path));
    if (!(file === root || file.startsWith(root + sep)) || path.includes("\0")) {
      res.writeHead(404).end();
      return;
    }
    let body;
    try {
      if (!statSync(file).isFile()) throw new Error("not a file");
      body = readFileSync(file);
    } catch {
      res.writeHead(404, { "content-type": "text/plain" }).end("not found\n");
      return;
    }
    res.writeHead(200, {
      "content-type": TYPES[extname(file)] ?? "application/octet-stream",
      "content-length": body.length,
      "cache-control": "no-store",
      "x-content-sha256": sha256(body),
      "x-robots-tag": "noindex",
    });
    res.end(req.method === "HEAD" ? undefined : body);
  };
}

function labPage({ out, baseUrl, last, error }) {
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const rows = last
    ? last.pages.map((p) => `<tr><td>${esc(p.layout)}</td><td><a href="${esc(p.url)}">${esc(p.url)}</a></td><td><code>${p.sha256.slice(0, 16)}</code></td></tr>`).join("")
    : "";
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Venue portal lab</title>
<style>body{font:15px system-ui;margin:2rem;max-width:56rem}fieldset{margin:1rem 0}textarea{width:100%;height:12rem;font:12px ui-monospace}td,th{padding:.25rem .5rem;text-align:left}.err{color:#b00}</style></head>
<body><h1>Venue portal lab <small>(localhost only)</small></h1>
<p>Pick a disruption. Pages under <code>${esc(baseUrl)}</code> change; the oracle is rewritten to <code>${esc(out)}/trusted/</code> and is never served.
Restart organization servers with the new <code>--oracle-suite</code> if they load suites at startup.</p>
${error ? `<p class="err">${esc(error)}</p>` : ""}
<form method="post" action="/generate">
<fieldset><legend>Preset</legend>
<label><input type="radio" name="preset" value="baseline" checked> Baseline (table + cards + missing price)</label><br>
<label><input type="radio" name="preset" value="capacity-cut"> Capacity cut: the hall that fit shrinks below the attendee count</label><br>
<label><input type="radio" name="preset" value="layout-drift"> Layout drift: unseen list and transposed layouts</label><br>
<label><input type="radio" name="preset" value="custom"> Custom spec (JSON below)</label></fieldset>
<label>Seed <input name="seed" value="${esc(last?.manifest.spec.seed ?? "genesis")}"></label>
<label>Public base URL <input name="base_url" size="48" value="${esc(baseUrl)}"></label>
<p><textarea name="spec" placeholder='{"suite_id": "...", "date": "2026-11-14", ...}'></textarea></p>
<button>Regenerate</button></form>
${last ? `<h2>Current variant: <code>${esc(last.suite.suite_id)}</code></h2><table><tr><th>Layout</th><th>URL</th><th>sha256</th></tr>${rows}</table>` : ""}
</body></html>`;
}

function readBody(req, limit = 256 * 1024) {
  return new Promise((ok, bad) => {
    let size = 0;
    const chunks = [];
    req.on("data", (c) => {
      size += c.length;
      if (size > limit) {
        bad(new Error("request too large"));
        req.destroy();
      } else chunks.push(c);
    });
    req.on("end", () => ok(Buffer.concat(chunks).toString("utf8")));
    req.on("error", bad);
  });
}

export function labHandler({ out, baseUrl: initialBase }) {
  let baseUrl = initialBase;
  let last = null;
  return async (req, res) => {
    const send = (code, html) => res.writeHead(code, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" }).end(html);
    if (req.method === "GET" && new URL(req.url, "http://x").pathname === "/") return send(200, labPage({ out, baseUrl, last }));
    if (req.method === "POST" && req.url === "/generate") {
      try {
        const form = new URLSearchParams(await readBody(req));
        const preset = form.get("preset") ?? "baseline";
        const seed = (form.get("seed") || "genesis").slice(0, 64);
        baseUrl = form.get("base_url") || baseUrl;
        const spec = preset === "custom" ? JSON.parse(form.get("spec") || "null") : presetSpec(preset, seed);
        if (spec && typeof spec === "object") spec.seed = seed;
        last = writeVariant({ spec, baseUrl, out });
        res.writeHead(303, { location: "/" }).end();
      } catch (e) {
        send(400, labPage({ out, baseUrl, last, error: e.message }));
      }
      return;
    }
    res.writeHead(404).end();
  };
}

function main() {
  const { values } = parseArgs({
    options: {
      out: { type: "string", default: "data/portal" },
      host: { type: "string", default: "127.0.0.1" },
      port: { type: "string", default: "7200" },
      "lab-port": { type: "string", default: "7201" },
      "base-url": { type: "string" },
      "no-lab": { type: "boolean" },
    },
  });
  const pub = join(values.out, "public");
  try {
    statSync(join(pub, "index.html"));
  } catch {
    console.error(`no portal at ${pub}; run generate.mjs first`);
    process.exit(2);
  }
  createServer(publicHandler(pub)).listen(Number(values.port), values.host, () =>
    console.log(`portal  http://${values.host}:${values.port}/  (serving ${resolve(pub)} only)`),
  );
  if (!values["no-lab"]) {
    let baseUrl = values["base-url"];
    if (!baseUrl) {
      try {
        baseUrl = JSON.parse(readFileSync(join(values.out, "trusted", "manifest.json"), "utf8")).base_url;
      } catch {
        baseUrl = `http://127.0.0.1:${values.port}`;
      }
    }
    createServer(labHandler({ out: values.out, baseUrl })).listen(Number(values["lab-port"]), "127.0.0.1", () =>
      console.log(`lab     http://127.0.0.1:${values["lab-port"]}/  (localhost only; do not tunnel)`),
    );
  }
}

if (import.meta.url === `file://${process.argv[1]}`) main();
