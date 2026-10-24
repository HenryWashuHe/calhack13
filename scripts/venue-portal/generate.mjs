#!/usr/bin/env node
// Generate a synthetic venue portal variant.
//
//   node scripts/venue-portal/generate.mjs --preset baseline --seed demo \
//        --base-url https://<tunnel>.trycloudflare.com --out data/portal
//
// Writes (one shared --out can hold many namespaces, served together):
//   <out>/public/<ns>/venues/*.html      served to Actors and judges (HTML only)
//   <out>/trusted/suites/<suite>.json    oracle suite for --oracle-suite (never served)
//   <out>/trusted/manifests/<ns>.json    spec, seed and digests (never served)
//   <out>/fragments/<ns>.json            public-safe allowed URLs + example HTML
//
// A namespace that already exists is never overwritten unless `replace` is
// set: organizations bind the exact page bytes and suite digest of a run, so
// changing served pages under an existing run breaks its evidence.
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { build, indexPage, namespaceIndex, presetSpec } from "./lib.mjs";

export function writeVariant({ spec, baseUrl, out, namespace = spec?.suite_id, replace = false }) {
  const root = resolve(out);
  const pub = join(root, "public");
  const suites = join(root, "trusted", "suites");
  const manifests = join(root, "trusted", "manifests");
  const result = build(spec, baseUrl, namespace);
  const nsDir = join(pub, namespace);
  const suitePath = join(suites, `${result.suite.suite_id}.json`);
  if (!replace && (existsSync(nsDir) || existsSync(suitePath))) {
    throw new Error(
      `namespace ${namespace} or suite ${result.suite.suite_id} already exists; pick a new seed/suite/namespace, or pass --replace only for a fresh organization database`,
    );
  }
  rmSync(nsDir, { recursive: true, force: true });
  mkdirSync(join(nsDir, "venues"), { recursive: true });
  mkdirSync(suites, { recursive: true });
  mkdirSync(manifests, { recursive: true });
  mkdirSync(join(root, "fragments"), { recursive: true });
  for (const p of result.pages) writeFileSync(join(pub, p.path), p.html);
  writeFileSync(join(nsDir, "index.html"), namespaceIndex(result.pages));
  const namespaces = readdirSync(pub, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name).sort();
  writeFileSync(join(pub, "index.html"), indexPage(namespaces));
  writeFileSync(suitePath, `${JSON.stringify(result.suite, null, 2)}\n`);
  writeFileSync(join(manifests, `${namespace}.json`), `${JSON.stringify(result.manifest, null, 2)}\n`);
  const fragmentPath = join(root, "fragments", `${namespace}.json`);
  writeFileSync(fragmentPath, `${JSON.stringify(result.fragment, null, 2)}\n`);
  return { ...result, suitePath, fragmentPath, publicDir: pub, namespace };
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
      namespace: { type: "string" },
      replace: { type: "boolean" },
      help: { type: "boolean", short: "h" },
    },
  });
  if (values.help || (!values.preset && !values.spec)) {
    console.log(`usage: generate.mjs (--preset baseline|capacity-cut|layout-drift [--seed S] | --spec file.json)
                    [--base-url URL] [--out DIR] [--suite-id ID] [--attendees N]
                    [--namespace NS] [--replace]`);
    process.exit(values.help ? 0 : 2);
  }
  const spec = values.spec ? JSON.parse(readFileSync(values.spec, "utf8")) : presetSpec(values.preset, values.seed);
  if (values["suite-id"]) spec.suite_id = values["suite-id"];
  if (values.attendees) spec.attendees = Number(values.attendees);
  const r = writeVariant({ spec, baseUrl: values["base-url"], out: values.out, namespace: values.namespace ?? spec.suite_id, replace: values.replace });
  for (const p of r.pages) console.log(`${p.sha256.slice(0, 12)}  ${p.layout.padEnd(10)} ${p.url}`);
  console.log(`\npublic : ${r.publicDir}`);
  console.log(`suite  : ${r.suitePath}   (trusted; pass to --oracle-suite, never serve)`);
  console.log(`mission: ${r.fragmentPath}   (combine A and B with combine-fragments.mjs)`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (e) {
    console.error(`error: ${e.message}`);
    process.exit(2);
  }
}
