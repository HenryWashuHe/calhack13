# Judge-controlled venue portal

The scenario's trusted suites and `mission.json` point at
`https://fixtures.genesis.example/...`, which does not resolve. A cloud Apify
Actor cannot fetch it, so the capability pipeline cannot run live against those
URLs. This portal generates fresh venue pages, serves them, and writes the
matching oracle suite to a separate trusted directory that is never served.

It also covers the research report's demonstration requirement: the judge varies
capacity, date, availability, price and layout without choosing the role or the
code GENESIS should create.

## What it produces

```sh
node scripts/venue-portal/generate.mjs --preset baseline --seed demo --namespace org-a \
  --base-url https://<your-tunnel>.trycloudflare.com --out data/portal
```

One `--out` holds any number of namespaces, all served together:

| Output | Served? | Use |
| --- | --- | --- |
| `data/portal/public/<ns>/venues/*.html`, `<ns>/examples/*.html` (with `--holdout`), `<ns>/index.html` (links every emitted page by its own path), `index.html` | yes | what the Actor and the judge read |
| `data/portal/trusted/suites/<suite_id>.json` | **never** | pass to `genesis org --oracle-suite` |
| `data/portal/trusted/manifests/<ns>.json` | **never** | spec, seed and digests for the record |
| `data/portal/fragments/<ns>.json` | no (public-safe) | `allowed_source_urls`, `examples` (HTML only; held out with `--holdout`) and a suggested `invocation_input` |

Page URLs are `<base>/<ns>/venues/<slug>.html`; the namespace defaults to the
suite ID. Page bytes never contain their own URL, so a new `--base-url` changes
URLs in the suite but not any `sha256`.

**An existing namespace is never overwritten** (the generator and the lab
refuse). Organizations bind the exact page bytes and the full suite digest of a
run; acquisition and invocation recovery reject a missing or changed suite. A
new variant therefore gets a new namespace and suite ID, and earlier suite files
stay on disk. `--replace` exists only for a fresh organization database.

## Presets

| Preset | What changes | Demo use |
| --- | --- | --- |
| `baseline` | table + cards (CAD) + a cards page with no numeric price | first acquisition; mirrors the checked-in fixture shapes with new values |
| `capacity-cut` | same rooms as `baseline` for the same seed; the hall that fit shrinks below the attendee count | the withheld second disruption, handled by the new responsibility |
| `layout-drift` | unseen prose `list` and attribute-per-row `transposed` layouts, suffix/grouped prices, EUR | forces a semantic repair, or shows Org B rejecting an incompatible capability |

Use `--seed` for a new variant the generator has never seen, `--attendees` for
an organization-specific input (Org B's suite must contain its exact invocation
input) and `--suite-id` to keep suites distinct. `--spec file.json` sets every
value directly: `suite_id`, `date`, `utc_offset`, `attendees`, and `pages[]`
with `slug`, `layout` (`table|cards|list|transposed`), optional `date`, optional
`missing: "price"`, and `venues[]` (`venue_id`, `name`, `capacity`,
`price_minor`, `currency` in `USD|CAD|EUR`, `start`/`end` as `HH:MM`,
`availability`, `step_free`). The verifier needs two distinct supported layouts
and one missing-price page; the generator refuses specs that don't meet that.

## Serve it publicly (no account)

```sh
cloudflared tunnel --url http://127.0.0.1:7200      # 1. prints https://<random>.trycloudflare.com
B=https://<random>.trycloudflare.com
node scripts/venue-portal/generate.mjs --preset baseline --seed demo --namespace org-a \
  --base-url $B --out data/portal                   # 2. pages + suite for that URL
node scripts/venue-portal/serve.mjs --out data/portal --base-url $B   # 3. portal :7200, lab :7201
```

Get the tunnel URL before generating: suites embed it. Quick tunnels need no
Cloudflare login. The URL changes every time the tunnel
restarts, and quick tunnels are for testing, with no uptime guarantee. Any
static host works the same way: upload `public/` only.

Point the tunnel at **7200 only**. The lab on 7201 binds `127.0.0.1` and lets
whoever is at the demo machine pick a preset, seed, base URL, namespace or
custom JSON spec. Each variant goes to a new namespace; the lab refuses to
overwrite one. Neither server reads `trusted/`.

## Wire it into a live scenario

```sh
B=https://<random>.trycloudflare.com
node scripts/venue-portal/generate.mjs --preset baseline --seed a1 --namespace org-a \
  --suite-id portal-a-v1 --base-url $B --out data/portal
node scripts/venue-portal/generate.mjs --preset baseline --seed b1 --namespace org-b \
  --suite-id portal-b-v1 --attendees 150 --base-url $B --out data/portal
node scripts/venue-portal/combine-fragments.mjs \
  --a data/portal/fragments/org-a.json --b data/portal/fragments/org-b.json > data/portal/mission-fragment.json
```

1. Both namespaces are served at once by one `serve.mjs` and one tunnel, at
   distinct URLs with their own bytes.
2. Start A with `--oracle-suite data/portal/trusted/suites/portal-a-v1.json`, B with
   `--oracle-suite data/portal/trusted/suites/portal-b-v1.json`.
3. The scenario spec has one allowlist, one examples list and one invocation
   input. `combine-fragments.mjs` gives `allowed_source_urls` = A ∪ B,
   A's `examples` (generation input) and B's `invocation_input` (B's suite
   contains exactly that input), plus `suite_a_id`/`suite_b_id`. It refuses
   fragments that share URLs.
4. For a new disruption outside the original frozen suite, generate it into a **new** namespace
   (for example `--preset capacity-cut --namespace org-a-cut`) and start a new
   organization run with that suite, keeping the earlier suites loaded for
   organizations whose history references them. Do not replace a namespace an
   existing run used.

### Prepare continuing CREAO monitoring

`prepare-monitoring.mjs` builds the optional two-cycle scenario from three
existing immutable namespaces: A's generation sources, B's baseline, and B's
capacity-cut update. Run it **before starting either organization**. The new B
suite contains all its baseline cases plus the selected update case; B verifies
both snapshots during adoption. This is configured snapshot coverage, not an
unseen future page or a live-availability claim.

Use explicit event facts rather than relying on randomized presets to satisfy
price, window and step-free constraints. This creates example local input files;
exclusive writes refuse to replace existing ones:

```sh
mkdir -p runtime
node --input-type=module <<'JS'
import { writeFileSync } from 'node:fs';
import { presetSpec } from './scripts/venue-portal/lib.mjs';
const baseline = presetSpec('baseline', 'monitoring-event');
Object.assign(baseline, { suite_id: 'monitor-baseline-v1', date: '2026-11-14', utc_offset: '-08:00', attendees: 150 });
baseline.pages[0].date = '2026-11-14';
Object.assign(baseline.pages[0].venues[0], {
  venue_id: 'event-hall', capacity: 180, price_minor: 75000, currency: 'USD',
  start: '18:00', end: '22:00', availability: 'available', step_free: true,
});
const update = structuredClone(baseline);
update.suite_id = 'monitor-update-v1';
update.pages[0].venues[0].capacity = 100;
const policy = {
  selected_venue_id: 'event-hall', attendees: 150,
  budget: { minor_units: 80000, currency: 'USD' },
  window: { start: '2026-11-14T18:00:00-08:00', end: '2026-11-14T22:00:00-08:00' },
  step_free_required: true,
};
for (const [name, value] of Object.entries({ baseline, update, policy })) {
  writeFileSync(`runtime/monitor-${name}.json`, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
}
JS
```

After obtaining the HTTPS portal URL in `U`, generate all three namespaces and
prepare a fresh setup directory **outside** `data/portal`:

```sh
node scripts/venue-portal/generate.mjs --preset baseline --seed monitor-a --holdout \
  --suite-id monitor-a-v1 --namespace monitor-a --base-url "$U" --out data/portal
node scripts/venue-portal/generate.mjs --spec runtime/monitor-baseline.json \
  --namespace monitor-b --base-url "$U" --out data/portal
node scripts/venue-portal/generate.mjs --spec runtime/monitor-update.json \
  --namespace monitor-b-update --base-url "$U" --out data/portal
node scripts/venue-portal/prepare-monitoring.mjs \
  --portal data/portal --a monitor-a --b monitor-b --update monitor-b-update \
  --policy runtime/monitor-policy.json --suite-id monitor-b-frozen-v1 \
  --template crates/genesis-server/examples/scenario/mission.json \
  --out runtime/monitoring-001
```

The helper checks generated manifests, suites, fragments and actual HTML bytes;
requires a feasible baseline and a same-event update failing only capacity; and
keeps A's synthesis examples separate from oracle answers. It emits:

- `runtime/monitoring-001/trusted/suite-b.json`: B's complete frozen suite.
- `runtime/monitoring-001/scenario.json`: the strict scenario spec, including
  the public Actor allowlist and `venue_monitoring` policy/input.
- `runtime/monitoring-001/setup.json`: file references, digests and coverage
  labels for inspection. This is not a scenario spec.

Start A with `--oracle-suite data/portal/trusted/suites/monitor-a-v1.json`, and
B with `--oracle-suite runtime/monitoring-001/trusted/suite-b.json`. Run
`genesis scenario --spec runtime/monitoring-001/scenario.json` with both
organization URLs and operator tokens as described in
[INHERITED_MISSION.md](INHERITED_MISSION.md). B needs the real CREAO runtime,
role provisioner, readiness integration and `monitor.venue` authority.

The template's candidate, repair and access flags are preserved. The checked-in
template leaves `provider_access_acknowledged` false: arrange B's permitted Actor
access before explicitly acknowledging it. Change candidate versions/Actor names
for a new run. Never publish the setup directory or trusted suites. Output paths
inside the portal, symlinked public trees, public trees exceeding 4,096 entries,
modified source bytes and existing output directories are refused; earlier source
artifacts stay unchanged.

### Held-out examples (`--holdout`)

By default the fragment's `examples` are the suite's own pages, which makes the
oracle run a reproducibility check, not held-out evaluation. Pass `--holdout`
to keep them apart:

```sh
node scripts/venue-portal/generate.mjs --preset baseline --seed a1 --namespace org-a \
  --suite-id portal-a-v1 --base-url $U --out data/portal --holdout
```

For every supported page this also serves `<ns>/examples/<slug>.html`: same
layout, title and date, freshly drawn venues (no venue name or ID shared with
the suite). The fragment's `examples` are only those pages, and
`allowed_source_urls` lists cases and examples. **The suite is byte-identical
to the run without `--holdout`**, so verification is unchanged; the generator
just never sees a page it is graded on. The manifest records
`held_out_examples: true`. Same formats, new data: this tests generalization
across data, not to a new layout (the `layout-drift` preset is for that).

## Checks

```sh
cargo build -p genesis-verification            # provides target/debug/genesis-verify
node --test scripts/venue-portal/portal.test.mjs
```

The tests cover determinism, exact digests, base-URL independence, preset
coverage, spec and calendar-date validation (leap-year controls), the
public/trusted split, path traversal, A and B served simultaneously by one
server with their own bytes, fragment combination, and the no-overwrite rule
for the generator and the lab, and held-out examples (disjoint from the suite
by URL and venue, readable by the reference extractor, within the forge's
synthesis limits, suite unchanged). An independent reference extractor reads only the rendered HTML and
must agree with the oracle for 15 variants, so a rendering bug fails a test
instead of producing a wrong oracle. Each preset's suite goes through the real
`genesis-verify`: a correct candidate (reordered, UTC times) is accepted, and a
wrong capacity or a changed snapshot is rejected. Without the binary that test
is skipped and says so. A generated suite also loads in `genesis org
--oracle-suite`, which rejects a suite without an unsupported case.

## Limits

Synthetic public data only; nothing here is a real venue, booking or
accessibility claim. A tunnel or static host proves reachability, not that a
particular Actor run happened: run IDs and receipts still come from Apify
through the backend.
