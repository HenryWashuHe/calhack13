"""Henry's Claude: Rust/Python differential for the Fetch venue replacement contract.

Generates random recovery contexts (fixture-shaped, with edge cases in times,
URLs, text, prices and ties), asks Rust for the provider input, runs the Python
specialist on it, then asks Rust to verify Python's answer. Reports:

  rust-ok/python-invalid : Rust would send it, the specialist refuses (recovery stalls)
  verify-<error>         : both accept, Rust rejects Python's answer
  verified               : agreement

Usage: python3 scripts/review/venue_recovery_diff.py [N] [SEED]
Only third-party imports of wire.py are stubbed; venue_selection/wire code runs as is.
"""
import json
import os
import random
import subprocess
import sys
import tempfile
import types
from collections import Counter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BRIDGE = os.path.join(ROOT, "services", "fetch-bridge")


def stub_third_party():
    def mod(name, **attrs):
        m = types.ModuleType(name)
        m.__dict__.update(attrs)
        sys.modules[name] = m
        return m

    class Model:
        @staticmethod
        def build_schema_digest(_):
            return "stub"

    class Protocol:
        digest = "stub"

        def __init__(self, **_):
            pass

        def on_message(self, **_):
            return lambda f: f

    mod("uagents", Model=Model, Protocol=Protocol)
    mod("ecdsa", SECP256k1=object(), VerifyingKey=object)
    mod("bech32", bech32_decode=None, convertbits=None)
    mod("pydantic")
    mod("pydantic.v1", StrictStr=str)


stub_third_party()
sys.path.insert(0, BRIDGE)
from genesis_fetch_bridge.venue_selection import select_venue  # noqa: E402
from genesis_fetch_bridge.wire import Invalid  # noqa: E402

FIXTURE = json.load(open(os.path.join(ROOT, "contracts/fixtures/fetch_venue_replacement_v1.json")))
BASE = FIXTURE["input"]["context"]

URLS_OK = [
    "https://fixtures.genesis.example/replacements.html",
    "https://Fixtures.Genesis.Example/a?b=c",
    "HTTPS://fixtures.genesis.example/x",
    "http://[::1]/v",
    "http://127.0.0.1:8080/v",
    "https://bücher.example/v",
    "https://fixtures.genesis.example:443/v",
    "http://h:/v",
    "https://fixtures.genesis.example/a%20b",
]
URLS_EDGE = [
    "http:///fixtures.genesis.example/v",
    "http:fixtures.genesis.example/v",
    "http:\\\\fixtures.genesis.example\\v",
    "http://:@fixtures.genesis.example/v",
    "http://@fixtures.genesis.example/v",
    "https://fixtures.genesis.example\\v",
    "http://fixtures.genesis.example./v",
    "https://exa mple.com/v",
    "https://h:99999/v",
    "https://h/v#frag",
    "ftp://h/v",
    "https:///",
    "http://%41.example/v",
    "http://0x7f.1/v",
]
TEXT_EDGE = [
    "alpha-hall", "Alpha-Hall", "é-hall", "e\u0301-hall", "hall-\u00e9", "大会堂",
    "hall\u00a0two", "hall\u200b", "\u200bhall", "hall\u2028x", "hall\u0085",
    "hall\u00a0", "\u3000hall", "hall\ufeff", "\ufeffhall", "hall\u180e", "\u180ehall",
    "hall\u2066x\u2069", "😀hall", "a" * 256, "é" * 128, "é" * 129, "hall\u001f",
    "hall\x7f", " hall", "hall\t",
]


def stamp(rng, base_hour):
    y = rng.choice([2026, 2028, 2032, 2100, 9999, 1, 2026])
    mo, d = rng.choice([(11, 14), (2, 29), (2, 28), (12, 31), (1, 1), (11, 31)])
    h = base_hour % 24
    frac = rng.choice(["", ".0", ".5", ".000000001", ".123456789", ".1234567890", "."])
    off = rng.choice(["Z", "z", "+00:00", "-00:00", "-08:00", "+05:30", "+23:59", "-23:59", "+24:00", "+01:60"])
    t = rng.choice(["T", "t"])
    return f"{y:04d}-{mo:02d}-{d:02d}{t}{h:02d}:{rng.choice([0, 30, 59]):02d}:{rng.choice([0, 59, 60]):02d}{frac}{off}"


def window(rng, edge):
    if not edge:
        return {"start": "2026-11-14T18:00:00Z", "end": "2026-11-14T22:00:00Z"}
    return {"start": stamp(rng, rng.randint(0, 12)), "end": stamp(rng, rng.randint(12, 23))}


def generate(rng):
    ctx = json.loads(json.dumps(BASE))
    edge = rng.random() < 0.5
    ctx["recovery_id"] = rng.choice(["recovery-1", "r:1.a_b", "x" * 128])
    pol = ctx["policy"]
    pol["attendees"] = rng.choice([1, 120, 121, 1_000_000])
    pol["budget"]["minor_units"] = rng.choice([0, 10000, 80000, 10**12])
    pol["step_free_required"] = rng.random() < 0.5
    if edge and rng.random() < 0.5:
        pol["window"] = window(rng, True)
    n = rng.randint(1, 16)
    cands = []
    sources = {}
    for i in range(n):
        venue_id = rng.choice(TEXT_EDGE) if edge and rng.random() < 0.3 else f"venue-{rng.randint(0, 30)}"
        if i == 0 and rng.random() < 0.3:
            venue_id = pol["selected_venue_id"]
        src = f"job_{rng.randint(0, 9)}"
        url = rng.choice(URLS_EDGE) if edge and rng.random() < 0.3 else rng.choice(URLS_OK)
        binding = sources.setdefault(src, (rng.choice("abcdef") * 64, url))
        v = {
            "venue_id": venue_id,
            "capacity": rng.choice([0, 119, 120, 121, 500, 2**32 - 1]),
            "price": {"minor_units": rng.choice([0, 9999, 10000, 10000, 80000, 80001, 2**63, 2**64 - 1]),
                      "currency": rng.choice(["USD", "USD", "USD", "EUR", "usd"])},
            "window": window(rng, edge and rng.random() < 0.5),
            "availability": rng.choice(["available", "available", "unavailable"]),
            "step_free": rng.random() < 0.7,
            "evidence": {"source_url": binding[1], "snapshot_sha256": "a" * 64,
                         "locator": rng.choice(["venue:x", "L" * 1024, "loc\u00e9"])},
        }
        cands.append({"source_job_id": src, "invocation_proof_ref": "invocation:" + binding[0], "venue": v})
    ctx["candidates"] = cands
    ctx["policy_sha256"] = "0" * 64  # Rust fills the real hash
    return ctx


TIMES = [
    "2026-11-15T02:00:00Z", "2026-11-15T02:00:00z", "2026-11-15t02:00:00Z",
    "2026-11-14T18:00:00-08:00", "2026-11-15T02:00:00+00:00", "2026-11-15T02:00:00-00:00",
    "2026-11-15T02:00:00.000000000Z", "2026-11-15T02:00:00.1234567890Z", "2026-11-15T02:00:00.Z",
    "2026-11-15T01:59:59.999999999Z", "2026-11-15T02:00:00.000000001Z", "2026-11-15T07:30:00+05:30",
    "2026-11-16T01:59:00+23:59", "2026-11-14T02:01:00-23:59", "2026-11-15T02:00:00+24:00",
    "2026-11-15T02:00:00+01:60", "2026-11-15T24:00:00Z", "2026-11-15T02:00:60Z", "2026-02-29T02:00:00Z",
    "2028-02-29T02:00:00Z", "2100-02-29T02:00:00Z", "2026-11-31T02:00:00Z", "0001-01-01T00:00:00Z",
    "0000-01-01T00:00:00Z", "9999-12-31T23:59:59Z", "2026-11-15T02:00:00", "2026-11-15 02:00:00Z",
    "+2026-11-15T02:00:00Z", "2026-11-15T02:00:00.123456789+00:00", "2026-11-15T02:00Z",
    "2026-11-15T02:00:00Ｚ", "２026-11-15T02:00:00Z", "2026-11-15T02:00:00.1٣Z",
]


def targeted():
    """One field changed per case, from the valid fixture."""
    cases = []
    def put(label, path, value):
        ctx = json.loads(json.dumps(BASE))
        # Own source job, so changing its evidence can't break the per-source
        # binding consistency that would mask the field under test.
        ctx["candidates"][1]["source_job_id"] = "job_target"
        ctx["candidates"][1]["invocation_proof_ref"] = "invocation:" + "c" * 64
        target = ctx
        for k in path[:-1]:
            target = target[k]
        target[path[-1]] = value
        ctx["policy_sha256"] = "0" * 64
        cases.append((label, ctx))
    for u in URLS_OK + URLS_EDGE:
        put(f"source_url={u!r}", ["candidates", 1, "venue", "evidence", "source_url"], u)
    for t in TEXT_EDGE:
        put(f"venue_id={t!r}", ["candidates", 1, "venue", "venue_id"], t)
        put(f"locator={t!r}", ["candidates", 1, "venue", "evidence", "locator"], t)
        put(f"source_job_id={t!r}", ["candidates", 1, "source_job_id"], t)
        put(f"selected_venue_id={t!r}", ["policy", "selected_venue_id"], t)
    for t in TIMES:
        put(f"venue.window.start={t!r}", ["candidates", 1, "venue", "window", "start"], t)
        put(f"policy.window.end={t!r}", ["policy", "window", "end"], t)
    for n in [0, 1, 2**31, 2**32 - 1, 2**32, -1, 1.0, True, "120"]:
        put(f"capacity={n!r}", ["candidates", 1, "venue", "capacity"], n)
    for n in [0, 2**53 + 1, 2**63 - 1, 2**63, 2**64 - 1, 2**64, -1, 1.5, 1e3]:
        put(f"price={n!r}", ["candidates", 1, "venue", "price", "minor_units"], n)
    for n in [0, 1, 10**12, 10**12 + 1, 2**63]:
        put(f"budget={n!r}", ["policy", "budget", "minor_units"], n)
    for n in [0, 1, 10**6, 10**6 + 1]:
        put(f"attendees={n!r}", ["policy", "attendees"], n)
    for c in ["USD", "usd", "US", "USDD", "ÜSD", "U​SD"]:
        put(f"currency={c!r}", ["candidates", 1, "venue", "price", "currency"], c)
    for r in ["recovery-1", "x" * 128, "x" * 129, "r/1", "r 1", "é"]:
        put(f"recovery_id={r!r}", ["recovery_id"], r)
    return cases


def cargo(phase, d):
    env = dict(os.environ, VENUE_DIFF_PHASE=phase, VENUE_DIFF_DIR=d)
    p = subprocess.run(["cargo", "test", "-q", "-p", "genesis-server", "--test", "review_venue_recovery_diff"],
                       cwd=ROOT, env=env, capture_output=True, text=True)
    if p.returncode != 0:
        sys.exit(p.stdout[-3000:] + p.stderr[-3000:])


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 2000
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    rng = random.Random(seed)
    d = tempfile.mkdtemp(prefix="venue-diff-")
    if os.environ.get("VENUE_DIFF_TARGETED"):
        labeled = targeted()
        labels = [l for l, _ in labeled]
        contexts = [c for _, c in labeled]
    else:
        contexts = [generate(rng) for _ in range(n)]
        labels = [str(i) for i in range(n)]
    json.dump(contexts, open(f"{d}/contexts.json", "w"), ensure_ascii=False)
    cargo("inputs", d)
    inputs = json.load(open(f"{d}/inputs.json"))
    outputs = []
    for e in inputs:
        if e["rust"] != "ok":
            outputs.append({"python": "skipped"})
            continue
        try:
            outputs.append({"output": select_venue(e["input"])})
        except Invalid as exc:
            outputs.append({"python": f"invalid: {exc}"})
    json.dump(outputs, open(f"{d}/outputs.json", "w"), ensure_ascii=False)
    cargo("verify", d)
    verdicts = json.load(open(f"{d}/verdicts.json"))
    tally = Counter()
    examples = {}
    divergent = []
    for label, e, o, v in zip(labels, inputs, outputs, verdicts):
        if e["rust"] != "ok":
            key = f"rust-rejects ({e['rust'] if e['rust'] != 'deserialize' else 'deserialize'})"
        elif "output" not in o:
            key = f"rust-ok/python-{o['python']}"
        else:
            key = "verified" if v == "verified" else f"verify-{v}"
        tally[key] += 1
        if key.startswith(("rust-ok/", "verify-")):
            divergent.append(f"{key}  <-  {label}")
        if key.startswith(("rust-ok/", "verify-")) and key not in examples:
            examples[key] = e["context"]
    print(json.dumps(dict(tally.most_common()), indent=1, ensure_ascii=False))
    if os.environ.get("VENUE_DIFF_TARGETED"):
        print("\n".join(divergent))
        examples = {}
    for k, ctx in examples.items():
        print(f"\n== example for {k}")
        print(json.dumps(ctx, ensure_ascii=False)[:1500])
    print(f"\nartifacts in {d}")


if __name__ == "__main__":
    main()
