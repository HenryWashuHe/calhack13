// Independent #203 CSS cascade guard. Node VM fixture rendering and source rules only; no browser/network.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

const html = fs.readFileSync(new URL("./index.html", import.meta.url), "utf8");
const source = html.slice(html.indexOf("<script>") + "<script>".length, html.lastIndexOf("</script>"));

function harness(fetchImpl) {
  const nodes = new Map();
  const docHandlers = {};
  const el = () => {
    const n = { textContent: "", innerHTML: "", value: "", children: [], handlers: {}, hidden: false, disabled: false, classes: new Set(),
      style: { setProperty(k, v) { this[k] = v; } }, setAttribute(k, v) { this["@" + k] = v; }, addEventListener(k, f) { this.handlers[k] = f; },
      appendChild(c) { this.children.push(c); }, replaceChildren(...c) { this.children = c; }, closest() { return null; }, focus() {} };
    Object.defineProperty(n, "className", { get() { return [...n.classes].join(" "); }, set(v) { n.classes = new Set(String(v).split(/\s+/).filter(Boolean)); } });
    n.classList = { add(c) { n.classes.add(c); }, remove(c) { n.classes.delete(c); }, toggle(c, on) { if (on === undefined) on = !n.classes.has(c); if (on) n.classes.add(c); else n.classes.delete(c); return on; }, contains(c) { return n.classes.has(c); } };
    return n;
  };
  const get = (id) => { if (!nodes.has(id)) nodes.set(id, el()); return nodes.get(id); };
  const intervals = new Map(); let id = 0;
  vm.runInNewContext(source, {
    document: { getElementById: get, createElement: el, querySelectorAll: () => get("dots").children, addEventListener(k, f) { docHandlers[k] = f; }, body: el(), documentElement: {}, fullscreenElement: null },
    fetch: fetchImpl, setTimeout: (f) => { f(); }, setInterval: (f) => { intervals.set(++id, f); return id; }, clearInterval: (k) => intervals.delete(k),
    Promise, Date, Math, Number, String, Object, Array, Set, Map, Error, JSON, encodeURIComponent, console,
  });
  for (const k of ["urlA", "urlB"]) get(k).value = "http://local-" + k;
  return { get, key: (k) => docHandlers.keydown({ key: k, target: null, preventDefault() {} }), submit: () => get("liveform").handlers.submit({ preventDefault() {} }), poll: () => { for (const f of intervals.values()) f(); } };
}

const css = html.slice(html.indexOf("<style>") + 7, html.indexOf("</style>")).replace(/\/\*[\s\S]*?\*\//g, "");
const vars = new Map([...css.matchAll(/(--[\w-]+)\s*:\s*([^;{}]+);/g)].map((m) => [m[1], m[2].trim()]));
const resolve = (v) => v.startsWith("var(") ? resolve(vars.get(v.slice(4, -1))) : v;
// Only simple pipeline class rules match these cells. Preserve CSS specificity and source order.
function pipelineStyle(node) {
  const matched = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].flatMap((m, index) => {
    const selector = m[1].trim();
    if (!/^\.pipeline \.stage(?:\.[a-z]+)*$/.test(selector)) return [];
    const required = selector.slice(".pipeline ".length).split(".").filter(Boolean);
    if (!required.every((c) => node.classes.has(c))) return [];
    return [{ specificity: required.length + 1, index, declarations: m[2] }];
  }).sort((a, b) => a.specificity - b.specificity || a.index - b.index);
  const out = {};
  for (const rule of matched)
    for (const declaration of rule.declarations.split(";")) {
      const separator = declaration.indexOf(":");
      if (separator >= 0) out[declaration.slice(0, separator).trim()] = declaration.slice(separator + 1).trim();
    }
  return out;
}
function contrast(foreground, background) {
  const luminance = (color) => {
    assert.match(color, /^#[0-9a-f]{6}$/i);
    const [r, g, b] = color.slice(1).match(/../g).map((v) => parseInt(v, 16) / 255)
      .map((v) => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
    return .2126 * r + .7152 * g + .0722 * b;
  };
  const a = luminance(foreground), b = luminance(background);
  return (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
}
function renderedRejection() {
  const h = harness(async () => { throw new Error("no network"); });
  h.get("playreplay").handlers.click();
  h.key("3");
  return h.get("pipeline").children.filter((n) => n.classes.has("rejected"));
}
function colors(node) {
  const style = pipelineStyle(node);
  const foreground = resolve(style.color);
  // The pipeline has no painted background; transparent cells reveal .mechanism's --ink-2.
  const background = style.background === "transparent" ? resolve("var(--ink-2)") : resolve(style.background);
  return { foreground, background, ratio: contrast(foreground, background) };
}

test("redesign control: the rejected forward stage remains legible", () => {
  const forward = renderedRejection().find((n) => !n.classes.has("branch"));
  assert.ok(forward);
  const c = colors(forward);
  assert.ok(c.ratio >= 4.5, JSON.stringify(c));
});

test("redesign: the actual terminal REJECTED cell must retain legible contrast", () => {
  const branch = renderedRejection().find((n) => n.textContent === "REJECTED");
  assert.ok(branch);
  assert.deepEqual([...branch.classes].sort(), ["branch", "rejected", "stage"]);
  const c = colors(branch);
  assert.ok(c.ratio >= 4.5, `terminal REJECTED color=${c.foreground} background=${c.background} contrast=${c.ratio.toFixed(3)}`);
});
