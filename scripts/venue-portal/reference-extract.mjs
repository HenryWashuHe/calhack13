// Independent reference extractor used only by tests.
//
// It reads rendered HTML and nothing from the generator's internal data, so a
// rendering bug (page says 150, oracle says 180) fails a test instead of
// producing a wrong oracle that would "verify" a wrong capability. It is not a
// general HTML parser: it understands exactly the four portal layouts.

const PRICE_PREFIX = /^([A-Z]{3}) ([\d,]+)\.(\d{2})$/;
const PRICE_SUFFIX = /^([\d,]+)\.(\d{2}) ([A-Z]{3})(?: per booking)?$/;

const strip = (s) => s.replace(/<[^>]+>/g, "").replace(/&amp;/g, "&").trim();
const times = (s) => [...s.matchAll(/<time datetime="([^"]+)">/g)].map((m) => m[1]);

function price(text) {
  let m = PRICE_PREFIX.exec(text);
  if (m) return { minor_units: Number(m[2].replace(/,/g, "")) * 100 + Number(m[3]), currency: m[1] };
  m = PRICE_SUFFIX.exec(text);
  if (m) return { minor_units: Number(m[1].replace(/,/g, "")) * 100 + Number(m[2]), currency: m[3] };
  return null;
}

function venue(id, cap, priceText, window, avail, stepFree) {
  return { id, capacity: Number(cap), price: price(priceText), priceText, window, availability: avail, step_free: stepFree };
}

function table(html) {
  return [...html.matchAll(/<tr id="([^"]+)">(.*?)<\/tr>/gs)].map(([, id, row]) => {
    const td = [...row.matchAll(/<td>(.*?)<\/td>/gs)].map((m) => m[1]);
    const [start, end] = times(td[3]);
    return venue(id, strip(td[1]), strip(td[2]), { start, end }, strip(td[4]) === "Available" ? "available" : "unavailable", strip(td[5]) === "Yes");
  });
}

function cards(html) {
  return [...html.matchAll(/<article id="([^"]+)">(.*?)<\/article>/gs)].map(([, id, body]) => {
    const dd = Object.fromEntries([...body.matchAll(/<dt>(.*?)<\/dt><dd>(.*?)<\/dd>/gs)].map((m) => [strip(m[1]), strip(m[2])]));
    const [start, end] = times(body);
    return venue(id, dd.Capacity, dd.Price, { start, end }, dd.Availability === "Available" ? "available" : "unavailable", dd["Step-free access"] === "Yes");
  });
}

function list(html) {
  return [...html.matchAll(/<li id="([^"]+)">(.*?)<\/li>/gs)].map(([, id, body]) => {
    const parts = body.split(" · ");
    const cap = /seats <b>(\d+)<\/b>/.exec(body)[1];
    const [start, end] = times(body);
    return venue(id, cap, strip(parts[2]), { start, end }, /\bOpen\b/.test(parts[4]) ? "available" : "unavailable", /step-free entrance/.test(parts[5]));
  });
}

function transposed(html) {
  const ids = [...html.matchAll(/<th id="([^"]+)" scope="col">/g)].map((m) => m[1]);
  const rows = Object.fromEntries(
    [...html.matchAll(/<tr><th scope="row">(.*?)<\/th>(.*?)<\/tr>/gs)].map(([, label, cells]) => [label, [...cells.matchAll(/<td>(.*?)<\/td>/gs)].map((m) => m[1])]),
  );
  return ids.map((id, i) => {
    const [start, end] = times(rows.Hours[i]);
    return venue(id, strip(rows["Max guests"][i]), strip(rows.Rate[i]), { start, end }, /open/.test(rows.Status[i]) ? "available" : "unavailable", strip(rows.Accessible[i]) === "step-free");
  });
}

export function detectLayout(html) {
  if (html.includes('<table class="matrix">')) return "transposed";
  if (html.includes('<ul class="listings">')) return "list";
  if (html.includes("<table>")) return "table";
  if (html.includes("<article ")) return "cards";
  return null;
}

/** Extract in the oracle's output shape, adding evidence for the given URL/digest. */
export function referenceExtract(html, url, digest) {
  const layout = detectLayout(html);
  if (!layout) return { status: "unsupported", reason: "unsupported_format" };
  const found = { table, cards, list, transposed }[layout](html);
  const evidence = (id) => ({ source_url: url, snapshot_sha256: digest, locator: `#${id}` });
  const missing = found.find((v) => v.price === null);
  if (missing) return { status: "unsupported", reason: "missing_required_data", evidence: evidence(missing.id) };
  return {
    status: "supported",
    venues: found.map((v) => ({
      venue_id: v.id,
      capacity: v.capacity,
      price: v.price,
      window: v.window,
      availability: v.availability,
      step_free: v.step_free,
      evidence: evidence(v.id),
    })),
  };
}
