// Judge-controlled synthetic venue portal.
//
// Renders public venue pages and, separately, the trusted oracle suite that
// genesis-verification checks candidates against. The two outputs are written
// to different directories; only `public/` is ever served. Page bytes do not
// contain their own URL, so a suite can be re-pointed at a new public base URL
// (for example a fresh tunnel) without changing any snapshot digest.

import { createHash } from "node:crypto";

export const LAYOUTS = ["table", "cards", "list", "transposed"];
const AVAILABILITY = ["available", "unavailable"];
const CURRENCIES = ["USD", "CAD", "EUR"];
const ID_RE = /^[a-z][a-z0-9-]{1,47}$/;
const SLUG_RE = /^[a-z0-9][a-z0-9-]{0,47}$/;
const TIME_RE = /^([01]\d|2[0-3]):[0-5]\d$/;
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const OFFSET_RE = /^[+-](0\d|1[0-4]):[0-5]\d$/;

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** sha256 of compact JSON with recursively sorted keys (matches json_sha256 in Rust). */
export function jsonSha256(value) {
  return sha256(Buffer.from(canonicalJson(value), "utf8"));
}

export function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    const keys = Object.keys(value).sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

// Small deterministic PRNG (mulberry32) so a seed fully determines a variant.
export function rng(seed) {
  let a = hashSeed(String(seed));
  const next = () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int: (lo, hi) => lo + Math.floor(next() * (hi - lo + 1)),
    pick: (items) => items[Math.floor(next() * items.length)],
  };
}

function hashSeed(s) {
  let h = 2166136261;
  for (const ch of s) h = Math.imul(h ^ ch.codePointAt(0), 16777619);
  return h >>> 0;
}

const FIRST = ["Grand", "Makers", "Lakeside", "Hill", "Harbor", "Cedar", "Summit", "Atrium", "Granite", "Willow", "Orchard", "Beacon", "Foundry", "Meridian", "Canyon", "Juniper"];
const SECOND = ["Hall", "Loft", "Room", "Auditorium", "Pavilion", "Commons", "Gallery", "Forum", "Studio", "Terrace", "Annex", "Ballroom"];

function venueName(r, used) {
  for (let i = 0; i < 200; i++) {
    const name = `${r.pick(FIRST)} ${r.pick(SECOND)}`;
    const id = name.toLowerCase().replace(/\s+/g, "-");
    if (!used.has(id)) {
      used.add(id);
      return { name, id };
    }
  }
  throw new Error("could not allocate a unique venue name");
}

function randomVenue(r, used, overrides = {}) {
  const { name, id } = venueName(r, used);
  const startHour = r.int(9, 18);
  const length = r.int(3, 5);
  return {
    venue_id: id,
    name,
    capacity: r.int(6, 30) * 10,
    price_minor: r.int(15, 140) * 1000 + r.pick([0, 0, 500, 2500, 9900]),
    currency: "USD",
    start: `${String(startHour).padStart(2, "0")}:00`,
    end: `${String(Math.min(startHour + length, 23)).padStart(2, "0")}:00`,
    availability: r.next() < 0.7 ? "available" : "unavailable",
    step_free: r.next() < 0.6,
    ...overrides,
  };
}

/**
 * Built-in scenarios. Each returns a complete spec. A judge can also pass a
 * spec file (see docs/VENUE_PORTAL.md) to choose every value directly.
 */
export function presetSpec(name, seed = "genesis") {
  const r = rng(`${name}:${seed}`);
  const used = new Set();
  const base = {
    suite_id: `portal-${name}-${String(seed).replace(/[^a-z0-9-]/gi, "").toLowerCase() || "seed"}-v1`,
    date: "2026-11-14",
    utc_offset: "-08:00",
    attendees: 120,
  };
  const page = (slug, layout, count, extra = {}, venueOverrides = []) => ({
    slug,
    layout,
    title: extra.title ?? `${layout} listing`,
    date: extra.date,
    missing: extra.missing,
    venues: Array.from({ length: count }, (_, i) => randomVenue(r, used, venueOverrides[i] ?? {})),
  });
  switch (name) {
    case "baseline":
      return {
        ...base,
        pages: [
          page("campus-rooms", "table", 3, { title: "Campus event rooms" }, [{ capacity: 180, availability: "available" }]),
          page("community-spaces", "cards", 2, { title: "Community spaces", date: "2026-11-15" }, [{ currency: "CAD" }, { currency: "CAD" }]),
          page("gallery-listing", "cards", 1, { title: "Gallery listing", date: "2026-11-16", missing: "price" }),
        ],
      };
    case "capacity-cut": {
      // The same rooms as baseline, but the hall that fit the event shrinks
      // below the attendee count: the withheld second disruption.
      const spec = presetSpec("baseline", seed);
      spec.suite_id = base.suite_id;
      const hall = spec.pages[0].venues[0];
      hall.capacity = Math.max(10, Math.floor((spec.attendees * 0.6) / 10) * 10);
      return spec;
    }
    case "layout-drift":
      return {
        ...base,
        pages: [
          page("events-board", "list", 3, { title: "Events board" }, [{ currency: "EUR" }]),
          page("room-matrix", "transposed", 3, { title: "Room matrix", date: "2026-11-15" }),
          page("pop-up-listing", "list", 1, { title: "Pop-up listing", date: "2026-11-16", missing: "price" }),
        ],
      };
    default:
      throw new Error(`unknown preset ${JSON.stringify(name)}; use baseline, capacity-cut or layout-drift`);
  }
}

/** A real calendar date in YYYY-MM-DD (rejects 2026-02-31, 2026-13-01). */
export function isCalendarDate(s) {
  if (typeof s !== "string" || !DATE_RE.test(s)) return false;
  const [y, m, d] = s.split("-").map(Number);
  const t = new Date(Date.UTC(y, m - 1, d));
  return t.getUTCFullYear() === y && t.getUTCMonth() === m - 1 && t.getUTCDate() === d;
}

export const NAMESPACE_RE = /^[a-z0-9][a-z0-9.-]{0,63}$/;

export function validateSpec(spec) {
  const fail = (path, msg) => {
    throw new Error(`invalid spec at ${path}: ${msg}`);
  };
  if (!spec || typeof spec !== "object") fail("$", "must be an object");
  if (!/^[a-z0-9][a-z0-9.-]{2,63}$/.test(spec.suite_id ?? "")) fail("suite_id", "3-64 chars of a-z, 0-9, '.', '-'");
  if (!isCalendarDate(spec.date)) fail("date", "a real calendar date, YYYY-MM-DD");
  if (!OFFSET_RE.test(spec.utc_offset ?? "")) fail("utc_offset", "like -08:00");
  if (!Number.isInteger(spec.attendees) || spec.attendees < 1 || spec.attendees > 100000) fail("attendees", "positive integer");
  if (!Array.isArray(spec.pages) || spec.pages.length === 0 || spec.pages.length > 32) fail("pages", "1-32 pages");
  const slugs = new Set();
  const ids = new Set();
  spec.pages.forEach((p, i) => {
    const at = `pages[${i}]`;
    if (!SLUG_RE.test(p.slug ?? "") || slugs.has(p.slug)) fail(`${at}.slug`, "unique slug of a-z, 0-9, '-'");
    slugs.add(p.slug);
    if (!LAYOUTS.includes(p.layout)) fail(`${at}.layout`, `one of ${LAYOUTS.join(", ")}`);
    if (p.date !== undefined && !isCalendarDate(p.date)) fail(`${at}.date`, "a real calendar date, YYYY-MM-DD");
    if (p.missing !== undefined && p.missing !== "price") fail(`${at}.missing`, `only "price" is supported`);
    if (!Array.isArray(p.venues) || p.venues.length === 0 || p.venues.length > 24) fail(`${at}.venues`, "1-24 venues");
    if (p.missing && p.venues.length !== 1) fail(`${at}.venues`, "a missing-data page holds exactly one venue");
    p.venues.forEach((v, j) => {
      const vt = `${at}.venues[${j}]`;
      if (!ID_RE.test(v.venue_id ?? "") || ids.has(v.venue_id)) fail(`${vt}.venue_id`, "unique id of a-z, 0-9, '-'");
      ids.add(v.venue_id);
      if (typeof v.name !== "string" || !/^[A-Za-z0-9 '&.-]{1,60}$/.test(v.name)) fail(`${vt}.name`, "1-60 plain characters");
      if (!Number.isInteger(v.capacity) || v.capacity < 1 || v.capacity > 1000000) fail(`${vt}.capacity`, "positive integer");
      if (!Number.isInteger(v.price_minor) || v.price_minor < 0 || v.price_minor > 1e12) fail(`${vt}.price_minor`, "non-negative integer minor units");
      if (!CURRENCIES.includes(v.currency)) fail(`${vt}.currency`, `one of ${CURRENCIES.join(", ")}`);
      if (!TIME_RE.test(v.start ?? "") || !TIME_RE.test(v.end ?? "") || v.start >= v.end) fail(`${vt}.start/end`, "HH:MM with start before end on the same day");
      if (!AVAILABILITY.includes(v.availability)) fail(`${vt}.availability`, "available or unavailable");
      if (typeof v.step_free !== "boolean") fail(`${vt}.step_free`, "boolean");
    });
  });
  const supported = spec.pages.filter((p) => !p.missing);
  const formats = new Set(supported.map((p) => p.layout));
  if (formats.size < 2 || !spec.pages.some((p) => p.missing)) {
    fail("pages", "the verifier requires two distinct supported layouts and one missing-price page");
  }
  return spec;
}

// ---------- rendering ----------

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const iso = (date, time, offset) => `${date}T${time}:00${offset}`;
const major = (minor) => `${Math.floor(minor / 100)}.${String(minor % 100).padStart(2, "0")}`;
const grouped = (minor) => `${Math.floor(minor / 100).toLocaleString("en-US")}.${String(minor % 100).padStart(2, "0")}`;
const offsetLabel = (o) => `UTC${o[0] === "-" ? "−" : "+"}${o.slice(1)}`;
const longDate = (d) =>
  new Date(`${d}T12:00:00Z`).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric", timeZone: "UTC" });

function timeEl(date, time, offset) {
  return `<time datetime="${iso(date, time, offset)}">${time}</time>`;
}

function shell(title, date, offset, body) {
  return `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="robots" content="noindex"><title>GENESIS synthetic ${esc(title)}</title></head>
<body>
  <h1>${esc(title)} — ${esc(longDate(date))}</h1>
  <p>This is public synthetic demo data for the GENESIS venue test portal. Times use ${offsetLabel(offset)}.</p>
${body}
</body>
</html>
`;
}

function priceText(v, missing, style) {
  if (missing === "price") return "Contact us";
  if (style === "suffix") return `${major(v.price_minor)} ${v.currency}`;
  if (style === "grouped") return `${v.currency} ${grouped(v.price_minor)}`;
  return `${v.currency} ${major(v.price_minor)}`;
}

const RENDER = {
  table(p, date, off) {
    const rows = p.venues
      .map(
        (v) =>
          `      <tr id="${v.venue_id}"><td>${v.venue_id}</td><td>${v.capacity}</td><td>${priceText(v, p.missing)}</td><td>${timeEl(date, v.start, off)}–${timeEl(date, v.end, off)}</td><td>${v.availability === "available" ? "Available" : "Unavailable"}</td><td>${v.step_free ? "Yes" : "No"}</td></tr>`,
      )
      .join("\n");
    return `  <table>
    <thead><tr><th>Venue ID</th><th>Capacity</th><th>Price</th><th>Window</th><th>Availability</th><th>Step-free access</th></tr></thead>
    <tbody>
${rows}
    </tbody>
  </table>`;
  },
  cards(p, date, off) {
    return p.venues
      .map(
        (v) => `  <article id="${v.venue_id}">
    <h2>${esc(v.name)}</h2>
    <dl><dt>Venue ID</dt><dd>${v.venue_id}</dd><dt>Capacity</dt><dd>${v.capacity}</dd><dt>Price</dt><dd>${priceText(v, p.missing)}</dd><dt>Availability</dt><dd>${v.availability === "available" ? "Available" : "Unavailable"}</dd><dt>Step-free access</dt><dd>${v.step_free ? "Yes" : "No"}</dd></dl>
    <p>Window: ${timeEl(date, v.start, off)} to ${timeEl(date, v.end, off)}</p>
  </article>`,
      )
      .join("\n");
  },
  // Prose listings: different labels, suffix currency, Open/Booked wording.
  list(p, date, off) {
    const items = p.venues
      .map(
        (v) =>
          `    <li id="${v.venue_id}"><strong>${esc(v.name)}</strong> <small>(ref ${v.venue_id})</small> · seats <b>${v.capacity}</b> · ${priceText(v, p.missing, "suffix")} per booking · ${timeEl(date, v.start, off)} until ${timeEl(date, v.end, off)} · ${v.availability === "available" ? "Open" : "Booked"} · ${v.step_free ? "step-free entrance" : "stairs only"}</li>`,
      )
      .join("\n");
    return `  <ul class="listings">
${items}
  </ul>`;
  },
  // One column per venue, one row per attribute; grouped thousands.
  transposed(p, date, off) {
    const head = p.venues.map((v) => `<th id="${v.venue_id}" scope="col">${esc(v.name)}</th>`).join("");
    const row = (label, cell) => `      <tr><th scope="row">${label}</th>${p.venues.map((v) => `<td>${cell(v)}</td>`).join("")}</tr>`;
    return `  <table class="matrix">
    <thead><tr><td></td>${head}</tr></thead>
    <tbody>
${[
  row("Reference", (v) => v.venue_id),
  row("Max guests", (v) => v.capacity),
  row("Rate", (v) => priceText(v, p.missing, "grouped")),
  row("Hours", (v) => `${timeEl(date, v.start, off)} → ${timeEl(date, v.end, off)}`),
  row("Status", (v) => (v.availability === "available" ? "✔ open" : "✖ taken")),
  row("Accessible", (v) => (v.step_free ? "step-free" : "no")),
].join("\n")}
    </tbody>
  </table>`;
  },
};

export function renderPage(spec, page) {
  const date = page.date ?? spec.date;
  return shell(page.title ?? page.slug, date, spec.utc_offset, RENDER[page.layout](page, date, spec.utc_offset));
}

export function formatId(page) {
  return page.missing ? `venue-${page.layout}-missing-${page.missing}-v1` : `venue-${page.layout}-v1`;
}

/** Normalize a public base URL: http(s), no query/fragment, no trailing slash. */
export function normalizeBaseUrl(raw) {
  let u;
  try {
    u = new URL(raw);
  } catch {
    throw new Error(`invalid --base-url ${JSON.stringify(raw)}`);
  }
  if (!/^https?:$/.test(u.protocol) || u.search || u.hash || u.username || u.password) {
    throw new Error("--base-url must be a plain http(s) URL without credentials, query or fragment");
  }
  return u.toString().replace(/\/+$/, "");
}

/**
 * Build every output in memory. Returns public pages (served), the trusted
 * suite and a trusted manifest (never served), and a public mission fragment.
 */
export function build(spec, baseUrl, namespace = spec.suite_id) {
  validateSpec(spec);
  if (!NAMESPACE_RE.test(namespace ?? "")) throw new Error("namespace must be 1-64 chars of a-z, 0-9, '.', '-'");
  const base = normalizeBaseUrl(baseUrl);
  const pages = [];
  const cases = [];
  for (const page of spec.pages) {
    const html = renderPage(spec, page);
    const bytes = Buffer.from(html, "utf8");
    const digest = sha256(bytes);
    const path = `${namespace}/venues/${page.slug}.html`;
    const url = `${base}/${path}`;
    const date = page.date ?? spec.date;
    pages.push({ slug: page.slug, path, url, html, sha256: digest, layout: page.layout });
    const evidence = (locator) => ({ source_url: url, snapshot_sha256: digest, locator });
    const window = { start: iso(date, "18:00", spec.utc_offset), end: iso(date, "22:00", spec.utc_offset) };
    const expected = page.missing
      ? { status: "unsupported", reason: "missing_required_data", evidence: evidence(`#${page.venues[0].venue_id}`) }
      : {
          status: "supported",
          venues: page.venues.map((v) => ({
            venue_id: v.venue_id,
            capacity: v.capacity,
            price: { minor_units: v.price_minor, currency: v.currency },
            window: { start: iso(date, v.start, spec.utc_offset), end: iso(date, v.end, spec.utc_offset) },
            availability: v.availability,
            step_free: v.step_free,
            evidence: evidence(`#${v.venue_id}`),
          })),
        };
    cases.push({
      case_id: page.slug,
      format_id: formatId(page),
      input: { source_url: url, attendees: spec.attendees, requested_window: window },
      source: { url, sha256: digest },
      expected,
    });
  }
  const layouts = [...new Set(spec.pages.map((p) => p.layout))].join(", ");
  const suite = {
    schema_version: 1,
    suite_id: spec.suite_id,
    scope: `Judge-generated synthetic venue extraction (${layouts}): exact venue data and an explicit unsupported missing price. Public synthetic data; no live availability, booking or safety claims.`,
    cases,
  };
  const manifest = {
    generated_by: "scripts/venue-portal",
    suite_id: spec.suite_id,
    namespace,
    base_url: base,
    spec,
    pages: pages.map(({ slug, url, sha256: s, layout }) => ({ slug, url, sha256: s, layout })),
  };
  const fragment = {
    note: "Public-safe. Paste into a scenario mission spec. Contains page HTML only, never expected answers.",
    suite_id: spec.suite_id,
    allowed_source_urls: pages.map((p) => p.url),
    examples: pages.filter((_, i) => !spec.pages[i].missing).map((p) => ({ url: p.url, html: p.html })),
    invocation_input: cases.find((c) => c.expected.status === "supported").input,
  };
  return { pages, suite, manifest, fragment };
}

/** Root index listing namespaces (directory names only). */
export function namespaceIndex(pages) {
  const items = pages.map((p) => `    <li><a href="venues/${esc(p.slug)}.html">${esc(p.slug)}</a> <code>${p.sha256.slice(0, 12)}</code></li>`).join("\n");
  return `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="robots" content="noindex"><title>GENESIS synthetic venue test portal</title></head>
<body>
  <h1>GENESIS synthetic venue test portal</h1>
  <p>Public synthetic demo data. Not a real booking site.</p>
  <ul>
${items}
  </ul>
</body>
</html>
`;
}

export function indexPage(namespaces) {
  const items = namespaces.map((n) => `    <li><a href="${esc(n)}/">${esc(n)}</a></li>`).join("\n");
  return `<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="robots" content="noindex"><title>GENESIS synthetic venue test portal</title></head>
<body>
  <h1>GENESIS synthetic venue test portal</h1>
  <p>Public synthetic demo data. Not a real booking site.</p>
  <ul>
${items}
  </ul>
</body>
</html>
`;
}
