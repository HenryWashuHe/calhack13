#!/usr/bin/env node
// Generate a synthetic venue portal variant.
//
//   node scripts/venue-portal/generate.mjs --preset baseline --seed demo \
//        --base-url https://<tunnel>.trycloudflare.com --out data/portal
//
// Writes:
//   <out>/public/              served to Actors and judges (HTML only)
//   <out>/trusted/<suite>.json  oracle suite for --oracle-suite (never served)
//   <out>/trusted/manifest.json spec, seed and digests (never served)
//   <out>/mission-fragment.json public-safe allowed URLs + example HTML
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, relative, resolve, isAbsolute } from "node:path";
import { parseArgs } from "node:util";
import { build, indexPage, presetSpec } from "./lib.mjs";

export function writeVariant({ spec, baseUrl, out }) {
  const root = resolve(out);
  const pub = join(root, "public");
  const trusted = join(root, "trusted");
  const rel = relative(pub, trusted);
  if (!rel.startsWith("..") && !isAbsolute(rel)) throw new Error("trusted directory must not be inside public/");
  const result = build(spec, baseUrl);
  rmSync(join(pub, "venues"), { recursive: true, force: true });
  mkdirSync(join(pub, "venues"), { recursive: true });
  mkdirSync(trusted, { recursive: true });
  for (const p of result.pages) writeFileSync(join(pub, p.path), p.html);
  writeFileSync(join(pub, "index.html"), indexPage(result.pages));
  const suitePath = join(trusted, `${result.suite.suite_id}.json`);
  writeFileSync(suitePath, `${JSON.stringify(result.suite, null, 2)}\n`);
  writeFileSync(join(trusted, "manifest.json"), `${JSON.stringify(result.manifest, null, 2)}\n`);
  writeFileSync(join(root, "mission-fragment.json"), `${JSON.stringify(result.fragment, null, 2)}\n`);
  return { ...result, suitePath, publicDir: pub };
}

function main() {
  const { values } = parseArgs({
    options: {
      preset: { type: "string" },
      spec: { type: "string" },
      seed: { type: "string", default: "genesis" },
      "base-url": { type: "string", default: "http://127.0.0.1:7200" },
      out: { type: "string", default: "data/portal" },
      "suite-id": { type: "string" },
      attendees: { type: "string" },
      help: { type: "boolean", short: "h" },
    },
  });
  if (values.help || (!values.preset && !values.spec)) {
    console.log(`usage: generate.mjs (--preset baseline|capacity-cut|layout-drift [--seed S] | --spec file.json)
                    [--base-url URL] [--out DIR] [--suite-id ID] [--attendees N]`);
    process.exit(values.help ? 0 : 2);
  }
  const spec = values.spec ? JSON.parse(readFileSync(values.spec, "utf8")) : presetSpec(values.preset, values.seed);
  if (values["suite-id"]) spec.suite_id = values["suite-id"];
  if (values.attendees) spec.attendees = Number(values.attendees);
  const r = writeVariant({ spec, baseUrl: values["base-url"], out: values.out });
  for (const p of r.pages) console.log(`${p.sha256.slice(0, 12)}  ${p.layout.padEnd(10)} ${p.url}`);
  console.log(`\npublic : ${r.publicDir}`);
  console.log(`suite  : ${r.suitePath}   (trusted; pass to --oracle-suite, never serve)`);
  console.log(`mission: ${join(resolve(values.out), "mission-fragment.json")}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (e) {
    console.error(`error: ${e.message}`);
    process.exit(2);
  }
}
