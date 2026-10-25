"""Analysis probe, not a proposed control: a development method that picks the
*kind* of cart from its own experiments, then refines it.

Hand-written by Henry's Claude to measure headroom: what does a method whose
output depends on the family reach under the same 72,000-step grant, compared
with random search (carts only) and with the parent's family-independent cart?
It never reads the family name; everything it learns comes from experiments.

  1. Screen six starting carts on two instances: two hand kinds (two large
     wooden wheels, five small rubber wheels) and four random carts drawn from
     a wider range than `random-search` uses (wheel radius up to 0.7, wood or
     rubber).
  2. Run the two best on the other four instances.
  3. Spend what is left on mutations of the best, tried first on the instances
     it fails; a mutation replaces it only if it solves more instances.
  4. Submit the best.

Same tool as `random-search`: the executor's line protocol on stdin/stdout,
  {"op":"instances"} -> public instances and remaining simulation steps
  {"op":"experiment","instance":id,"design":{...}} -> outcome, metrics, steps
  {"op":"submit","design":{...}} -> the one retained output
It is never a generated or improved method and says nothing about recursive
benefit.
"""
import json
import random
import sys


def call(request):
    sys.stdout.write(json.dumps(request) + "\n")
    sys.stdout.flush()
    line = sys.stdin.readline()
    if not line:
        sys.exit(3)
    return json.loads(line)


def build(p, rules):
    """A cart from parameters, placed at the right edge of the build zone."""
    x0, y0, x1, y1 = rules["zone"]
    w, h, r, n = p["w"], p["h"], p["r"], p["n"]
    right = min(x1 - 0.05, -0.05)
    w = min(w, right - x0 - 0.1)
    r = min(r, (w / 2) - 0.01, (y1 - max(y0 + 0.05, 0.0) - h - 0.05) / 2)
    cx = right - w / 2
    cy = max(y0 + 0.05, 0.0) + 2 * r + h / 2 + 0.02
    wheel_y = cy - h / 2
    bodies = [{"shape": "rect", "id": "deck", "x": cx, "y": cy, "w": w, "h": h, "material": "wood"}]
    joints = []
    xs = [cx - w / 2 + r + i * (w - 2 * r) / max(n - 1, 1) for i in range(n)]
    for i, x in enumerate(xs):
        bodies.append({"shape": "circle", "id": f"w{i}", "x": x, "y": wheel_y, "r": r,
                       "material": p["material"]})
        joints.append({"a": "deck", "b": f"w{i}", "kind": "hinge", "x": x, "y": wheel_y,
                       "motor_speed": p["speed"]})
    return {"language": 1, "payload_on": "deck", "bodies": bodies, "joints": joints}


HAND = [
    {"w": 3.0, "h": 0.2, "r": 0.6, "n": 2, "speed": -6.0, "material": "wood"},
    {"w": 4.0, "h": 0.25, "r": 0.25, "n": 5, "speed": -4.0, "material": "rubber"},
]


def sample(rng):
    return {"w": rng.uniform(1.2, 4.0), "h": rng.uniform(0.15, 0.3), "r": rng.uniform(0.12, 0.7),
            "n": rng.randint(2, 5), "speed": -rng.uniform(2.0, 9.0),
            "material": rng.choice(["wood", "rubber"])}


def mutate(rng, p):
    q = dict(p)
    q["w"] = min(4.0, max(1.0, p["w"] * rng.uniform(0.85, 1.15)))
    q["h"] = min(0.35, max(0.12, p["h"] * rng.uniform(0.85, 1.15)))
    q["r"] = min(0.8, max(0.1, p["r"] * rng.uniform(0.8, 1.25)))
    q["speed"] = min(-1.0, max(-10.0, p["speed"] * rng.uniform(0.8, 1.25)))
    if rng.random() < 0.25:
        q["n"] = min(6, max(2, p["n"] + rng.choice([-1, 1])))
    if rng.random() < 0.1:
        q["material"] = "rubber" if p["material"] == "wood" else "wood"
    return q


class Budget:
    def __init__(self, instances, remaining):
        self.by_id = {i["id"]: i for i in instances}
        self.episode = max(i["max_steps"] for i in instances)
        self.remaining = remaining

    def can(self):
        return self.remaining >= self.episode

    def trial(self, p, inst_id):
        """True/False for success, None when no budget is left."""
        if not self.can():
            return None
        inst = self.by_id[inst_id]
        reply = call({"op": "experiment", "instance": inst_id,
                      "design": build(p, inst["world"]["rules"])})
        if not reply.get("ok"):
            self.remaining = 0
            return None
        self.remaining = reply.get("remaining_steps") or 0
        m = reply.get("metrics") or {}
        return reply.get("outcome") == "scored" and bool(m.get("success"))


def main():
    rng = random.Random(20261025)
    info = call({"op": "instances"})
    instances = info["instances"]
    ids = [i["id"] for i in instances]
    b = Budget(instances, info["remaining_steps"] or 0)
    results = {}  # candidate index -> {instance id: success}
    cands = [dict(p) for p in HAND] + [sample(rng) for _ in range(4)]

    def run(k, which):
        for i in which:
            if i in results.setdefault(k, {}):
                continue
            ok = b.trial(cands[k], i)
            if ok is None:
                return
            results[k][i] = ok

    def wins(k):
        return sum(results.get(k, {}).values())

    for k in range(len(cands)):
        run(k, ids[:2])
    top = sorted(range(len(cands)), key=lambda k: -wins(k))[:2]
    for k in top:
        run(k, ids[2:])
    best = max(top, key=wins)
    while b.can() and wins(best) < len(ids):
        q = mutate(rng, cands[best])
        cands.append(q)
        k = len(cands) - 1
        failed = [i for i in ids if not results[best].get(i)]
        solved = [i for i in ids if results[best].get(i)]
        run(k, failed)
        if wins(k) == 0:
            continue
        run(k, solved)
        if len(results[k]) == len(ids) and wins(k) > wins(best):
            best = k
    call({"op": "submit", "design": build(cands[best], instances[0]["world"]["rules"])})


if __name__ == "__main__":
    main()
