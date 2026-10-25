"""genesis-lab cart adapter, split out of generated method 8215c367.

Everything domain-specific lives here and nothing searches: the executor's
line protocol, the instance features, the candidate space and how a point of
that space becomes a design. Bodies of `num`, `find` and `build` are the
parent's, unchanged.
"""
import json
import math
import sys

MAT = {'wood': (1.0,), 'rubber': (2.0,)}


def send(req):
    try:
        sys.stdout.write(json.dumps(req, separators=(',', ':')) + '\n')
        sys.stdout.flush()
        line = sys.stdin.readline()
        if not line:
            return None
        r = json.loads(line)
        return r if isinstance(r, dict) else None
    except Exception:
        return None


def num(v, d=0.0):
    return float(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else d


def find(w, key):
    if not isinstance(w, dict):
        return 0.0
    if key in w:
        return num(w[key])
    for k in ('params', 'parameters', 'drawn'):
        if isinstance(w.get(k), dict) and key in w[k]:
            return num(w[k][key])
    return 0.0


def build(r, speed, L, wm):
    wb = L - 0.4
    xf = -0.05 - r
    xr = xf - wb
    if xr - r < -5.95:
        return None, 0
    h = 0.2
    y = r + 0.01
    dx = (xr + xf) / 2
    deck = {'shape': 'rect', 'id': 'deck', 'x': round(dx, 4), 'y': round(y - 0.02 + h / 2, 4),
            'w': round(L, 4), 'h': h, 'angle': 0.0, 'material': 'wood'}
    bodies = [deck]
    joints = []
    for i, xw in enumerate((xr, xf)):
        bodies.append({'shape': 'circle', 'id': 'w%d' % i, 'x': round(xw, 4), 'y': round(y, 4), 'r': r, 'material': wm})
        joints.append({'a': 'deck', 'b': 'w%d' % i, 'kind': 'hinge', 'x': round(xw, 4), 'y': round(y, 4), 'motor_speed': speed})
    cost = L * h * 1.0 + 2 * math.pi * r * r * MAT[wm][0]
    return {'language': 1, 'payload_on': 'deck', 'bodies': bodies, 'joints': joints}, cost


def default():
    return build(0.6, -6.0, 3.6, 'rubber')[0]


def hardness(item):
    """Instance feature the strategy orders by. Ravine fields only: on
    rough-transport both are 0.0 for every instance, so the order is a no-op."""
    w = item.get('world') or {}
    return find(w, 'gap') + find(w, 'step_height')


def budget(items):
    b = None
    for i in items:
        v = ((i.get('world') or {}).get('rules') or {}).get('budget')
        if isinstance(v, (int, float)):
            b = v if b is None else min(b, v)
    return b


# Wheel radius, motor speed, deck length, wheel material.
AXES = ((0.6, 0.8, 0.45, 1.0), (-6.0, -10.0, -3.0), (3.0, 3.8), ('rubber', 'wood'))


def realizer(cap):
    def realize(point):
        d, c = build(*point)
        if d is None:
            return None
        if cap is not None and c > cap:
            return None
        return d
    return realize


class Session:
    """Protocol state: remaining steps, call count, the halt flag."""

    def __init__(self, limit):
        self.limit = limit
        self.remaining = 0
        self.calls = 0
        self.stopped = False

    def items(self):
        rep = send({'op': 'instances'})
        if rep and rep.get('ok'):
            self.remaining = num(rep.get('remaining_steps'))
            return rep.get('instances') or []
        return []

    def halted(self):
        return self.stopped

    def evaluate(self, item, design):
        ms = num(item.get('max_steps'), 2400)
        if self.calls >= self.limit or self.remaining < ms:
            self.stopped = True
            return None
        self.calls += 1
        r = send({'op': 'experiment', 'instance': item.get('id'), 'design': design})
        if not r or not r.get('ok'):
            self.stopped = True
            return None
        rs = r.get('remaining_steps')
        if isinstance(rs, (int, float)):
            self.remaining = rs
        else:
            self.remaining -= num(r.get('steps'))
        oc = r.get('outcome')
        if oc == 'unknown':
            self.stopped = True
        if oc != 'scored':
            return (False, 0.0)
        m = r.get('metrics') or {}
        return (bool(m.get('success')), num(m.get('progress')))

    def submit(self, design):
        send({'op': 'submit', 'design': design})
