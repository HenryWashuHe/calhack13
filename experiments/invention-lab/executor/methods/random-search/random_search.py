"""Seed development method: random search over wheeled carts. CONTROL ONLY.

Hand-written by Henry's Claude to exercise the executor end to end. It is never
a generated or improved method and its results say nothing about recursive
benefit. Its only tool is the executor's line protocol on stdin/stdout:
  {"op":"instances"} -> public instances and remaining simulation steps
  {"op":"experiment","instance":id,"design":{...}} -> outcome, metrics, steps
  {"op":"submit","design":{...}} -> the one retained output
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


def cart(rng, rules):
    x0, y0, x1, y1 = rules["zone"]
    w = rng.uniform(1.2, 4.0)
    h = rng.uniform(0.15, 0.3)
    r = rng.uniform(0.12, 0.4)
    n = rng.randint(2, 5)
    speed = -rng.uniform(2.0, 9.0)
    right = min(x1 - 0.05, -0.05)
    cx = right - w / 2
    cy = max(y0 + 0.05, 0.0) + 2 * r + h / 2 + 0.02
    wheel_y = cy - h / 2
    bodies = [{"shape": "rect", "id": "deck", "x": cx, "y": cy, "w": w, "h": h, "material": "wood"}]
    joints = []
    xs = [cx - w / 2 + r + i * (w - 2 * r) / max(n - 1, 1) for i in range(n)]
    for i, x in enumerate(xs):
        bodies.append({"shape": "circle", "id": f"w{i}", "x": x, "y": wheel_y, "r": r, "material": "rubber"})
        joints.append({"a": "deck", "b": f"w{i}", "kind": "hinge", "x": x, "y": wheel_y, "motor_speed": speed})
    return {"language": 1, "payload_on": "deck", "bodies": bodies, "joints": joints}


def score(reply):
    m = reply.get("metrics") or {}
    if reply.get("outcome") != "scored":
        return -1.0
    return (1.0 if m.get("success") else 0.0) + float(m.get("progress", 0.0))


def main():
    rng = random.Random(20261024)
    info = call({"op": "instances"})
    instances = info["instances"]
    episode = max(i["max_steps"] for i in instances)
    remaining = info["remaining_steps"] or 0
    best, best_score = None, -2.0
    # Spend about two thirds on exploration, the rest confirming the leaders.
    explore = remaining * 2 // 3
    leaders = []
    while explore >= episode and remaining >= episode:
        inst = rng.choice(instances)
        design = cart(rng, inst["world"]["rules"])
        reply = call({"op": "experiment", "instance": inst["id"], "design": design})
        if not reply.get("ok"):
            break
        remaining = reply.get("remaining_steps") or 0
        leaders.append((score(reply), design))
        explore -= episode
    leaders.sort(key=lambda t: -t[0])
    for _, design in leaders[:3]:
        total = 0.0
        for inst in instances:
            if remaining < episode:
                break
            reply = call({"op": "experiment", "instance": inst["id"], "design": design})
            if not reply.get("ok"):
                break
            remaining = reply.get("remaining_steps") or 0
            total += score(reply)
        if total > best_score:
            best, best_score = design, total
    if best is None and leaders:
        best = leaders[0][1]
    if best is None:
        sys.exit(2)
    call({"op": "submit", "design": best})


if __name__ == "__main__":
    main()
