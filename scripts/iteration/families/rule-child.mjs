// The child half of families/sandbox.mjs: runs with an empty environment and an empty cwd, reads one
// request from stdin, scores in-process (the vm timeouts still apply inside), writes one JSON score.
// Only allow-listed family modules can be loaded; the request names them, never a path.
const FAMILIES = { jobshop: () => import("./jobshop.mjs"), binpack: () => import("./binpack.mjs") };

const chunks = [];
process.stdin.on("data", (c) => chunks.push(c));
process.stdin.on("end", async () => {
  try {
    const req = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    const load = FAMILIES[req.family];
    if (!load) throw new Error(`unknown family ${String(req.family).slice(0, 40)}`);
    const mod = await load();
    const score = mod.scoreInProcess(req.source, req.instance, { ...req.options, timeoutMs: req.timeoutMs });
    process.stdout.write(JSON.stringify(score));
  } catch (err) {
    process.stdout.write(JSON.stringify({ success: false, progress: 0, cost: 1, detail: { error: (err && typeof err.message === "string" ? err.message : "child failed").slice(0, 160) } }));
  }
});
