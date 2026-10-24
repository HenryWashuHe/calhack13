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
node scripts/venue-portal/generate.mjs --preset baseline --seed demo \
  --base-url https://<your-tunnel>.trycloudflare.com --out data/portal
```

| Output | Served? | Use |
| --- | --- | --- |
| `data/portal/public/venues/*.html`, `index.html` | yes | what the Actor and the judge read |
| `data/portal/trusted/<suite_id>.json` | **never** | pass to `genesis org --oracle-suite` |
| `data/portal/trusted/manifest.json` | **never** | spec, seed and digests for the record |
| `data/portal/mission-fragment.json` | no (public-safe) | `allowed_source_urls`, `examples` (HTML only) and a suggested `invocation_input` for a scenario spec |

Page bytes never contain their own URL, so regenerating with a new
`--base-url` changes the suite's URLs but not any `sha256`.

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
node scripts/venue-portal/serve.mjs --out data/portal           # portal :7200, lab :7201
cloudflared tunnel --url http://127.0.0.1:7200                   # prints https://<random>.trycloudflare.com
node scripts/venue-portal/generate.mjs --preset baseline --seed demo \
  --base-url https://<random>.trycloudflare.com --out data/portal   # re-point the suite
```

Quick tunnels need no Cloudflare login. The URL changes every time the tunnel
restarts, and quick tunnels are for testing, with no uptime guarantee. Any
static host works the same way: upload `public/` only.

Point the tunnel at **7200 only**. The lab on 7201 binds `127.0.0.1` and lets
whoever is at the demo machine pick a preset, seed, base URL or custom JSON spec
and regenerate in place. It never reads or serves `trusted/`.

## Wire it into a live scenario

1. Generate A's variant and B's variant with different seeds, suite IDs and
   (for B) its own `--attendees`, both with the tunnel's base URL.
2. Start A with `--oracle-suite data/portal-a/trusted/<a>.json`, B with
   `--oracle-suite data/portal-b/trusted/<b>.json`.
3. Copy `allowed_source_urls`, `examples` and `invocation_input` from each
   `mission-fragment.json` into the scenario spec, and set `suite_a_id`/`suite_b_id`.
4. Organization servers load suites at startup: after regenerating in the lab,
   restart them with the new suite path.

Keep `examples` and suite cases apart for a held-out check: give the generator
one seed's pages as examples and evaluate on another seed's pages with the same
layouts. A shared seed is a reproducibility check, not held-out evaluation.

## Checks

```sh
cargo build -p genesis-verification            # provides target/debug/genesis-verify
node --test scripts/venue-portal/portal.test.mjs
```

The tests cover determinism, exact digests, base-URL independence, preset
coverage, spec validation, the public/trusted split, and path traversal against
the server. An independent reference extractor reads only the rendered HTML and
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
