#!/usr/bin/env node
// Eric's Claude: build one self-contained HTML file of the invention-lab board
// with the committed evidence embedded (docs/invention-lab/ui.md#results-board).
// Reads docs/invention-lab/evidence; writes only --out. No network access.
//
//   node apps/web/src/invention-lab/export-board.mjs [--out FILE] [--a ORG] [--b ORG]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../../../..");
const arg = (name, dflt) => {
  const i = process.argv.indexOf(name);
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : dflt;
};
const out = path.resolve(arg("--out", path.join(repo, "apps/web/dist/invention-lab-board.html")));
const orgA = arg("--a", "org_example_a");
const orgB = arg("--b", "org_example_b");
const evidence = path.join(repo, "docs/invention-lab/evidence");

const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true }).sort((x, y) => x.name.localeCompare(y.name))) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (!/\.(md|txt)$|SHA256SUMS$/.test(e.name)) files.push({ name: path.relative(evidence, p), text: fs.readFileSync(p, "utf8") });
  }
})(evidence);

const { build } = await import(process.env.LAB_ESBUILD ?? "esbuild");
const result = await build({
  entryPoints: [path.join(here, "board-entry.tsx")],
  bundle: true,
  write: false,
  minify: true,
  format: "iife",
  jsx: "automatic",
  outdir: "out",
  loader: { ".woff": "empty", ".woff2": "empty" },
  define: { "process.env.NODE_ENV": '"production"' },
  plugins: [
    {
      name: "lab-evidence",
      setup(b) {
        b.onResolve({ filter: /^virtual:lab-evidence$/ }, () => ({ path: "lab-evidence", namespace: "lab" }));
        b.onLoad({ filter: /.*/, namespace: "lab" }, () => ({
          contents: `export default ${JSON.stringify(files)}; export const orgA = ${JSON.stringify(orgA)}; export const orgB = ${JSON.stringify(orgB)};`,
          loader: "js",
        }));
      },
    },
  ],
});
const js = result.outputFiles.find((f) => f.path.endsWith(".js")).text.replace(/<\/script/gi, "<\\/script");
const css = result.outputFiles.find((f) => f.path.endsWith(".css"))?.text ?? "";
const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light dark"><title>GENESIS invention lab: A → B → A records</title><style>${css}</style></head>
<body><div id="root"></div><script>${js}</script></body></html>`;
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, html);
console.log(`wrote ${out} (${files.length} evidence files, ${Math.round(html.length / 1024)} KiB)`);
