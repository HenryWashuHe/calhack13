"""Stdlib-only port of two pieces of experiments/invention-lab/evaluator:

- family.rs `instance` and `instance_digest`: rebuild the world a retained
  plan names and check it against the plan's sha256;
- design.rs `Design::canonical`: the normalized design bytes whose sha256 the
  executor records for every experiment and submission.

Used only to replay retained evidence offline. The Rust crate is authoritative;
every use here is checked against a digest it produced.
"""
import hashlib
import json
import math
import struct
from decimal import Decimal

M = (1 << 64) - 1
FAMILIES = ('ravine-crossing', 'rough-transport', 'return-mix', 'projectile-target')


def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def f32s(x):
    """serde_json's f32 text: shortest digits that round-trip as f32."""
    v = f32(x)
    for p in range(1, 10):
        s = '%.*g' % (p, v)
        if f32(float(s)) == v:
            break
    s = format(Decimal(s), 'f')
    return s if '.' in s else s + '.0'


class Rng:
    def __init__(self, s):
        self.s = s & M

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & M
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M
        return ((z ^ (z >> 31)) >> 11) / float(1 << 53)

    def range(self, lo, hi):
        lo, hi = f32(lo), f32(hi)
        t = (lo + (hi - lo) * self.next()) * 1000.0
        r = math.floor(t + 0.5) if t >= 0 else -math.floor(-t + 0.5)  # Rust f64::round
        return f32(r / 1000.0)

    def int(self, lo, hi):
        return lo + int(math.floor((hi - lo + 1) * self.next()))


def instance(family, seed):
    r = Rng(seed ^ (((FAMILIES.index(family) + 1) * 0xA24BAED4963EE407) & M))
    i = dict(family=family, version=1, seed=seed,
             rules=dict(zone=[-6.0, 0.0, 0.0, 3.0], budget=0.0),
             payload_kg=0.0, friction=0.0, gap=0.0, step_height=0.0,
             bump_height=0.0, bump_spacing=0.0, distance=0.0,
             target_x=0.0, target_half_width=0.0, pushes=[])

    def pushes(hi):
        for _ in range(r.int(1, hi)):
            i['pushes'].append(dict(step=r.int(120, 1800), dx=r.range(-8.0, 8.0), dy=r.range(0.0, 5.0)))

    if family in ('ravine-crossing', 'return-mix'):
        i['gap'] = r.range(1.0, 2.5)
        i['step_height'] = r.range(-0.2, 0.2)
        i['payload_kg'] = r.range(5.0, 30.0)
        i['rules']['budget'] = r.range(4.0, 10.0)
        i['friction'] = r.range(0.5, 1.0)
        if family == 'return-mix':
            i['friction'] = r.range(0.3, 0.8)
            pushes(3)
    elif family == 'rough-transport':
        i['bump_height'] = r.range(0.1, 0.35)
        i['bump_spacing'] = r.range(0.6, 1.5)
        i['distance'] = 8.0
        i['payload_kg'] = r.range(5.0, 30.0)
        i['rules']['budget'] = r.range(4.0, 10.0)
        i['friction'] = r.range(0.5, 1.0)
        pushes(4)
    else:
        i['target_x'] = r.range(4.0, 9.0)
        i['target_half_width'] = 0.5
        i['payload_kg'] = r.range(1.0, 5.0)
        i['rules']['budget'] = r.range(4.0, 10.0)
        i['friction'] = r.range(0.5, 1.0)
    return i


def max_steps(family):
    return 1200 if family == 'projectile-target' else 2400


def _enc(o, floats, key=None):
    if isinstance(o, dict):
        return '{' + ','.join(json.dumps(k) + ':' + _enc(v, floats, k) for k, v in o.items()) + '}'
    if isinstance(o, (list, tuple)):
        return '[' + ','.join(_enc(v, floats, key) for v in o) + ']'
    if o is None or isinstance(o, bool):
        return json.dumps(o)
    if key in floats:
        return f32s(o)
    if isinstance(o, int):
        return str(o)
    return json.dumps(o, ensure_ascii=False)


WORLD_F32 = {'zone', 'budget', 'payload_kg', 'friction', 'gap', 'step_height', 'bump_height',
             'bump_spacing', 'distance', 'target_x', 'target_half_width', 'dx', 'dy'}


def instance_digest(world):
    return hashlib.sha256(_enc(world, WORLD_F32).encode()).hexdigest()


DESIGN_F32 = {'x', 'y', 'w', 'h', 'angle', 'r', 'motor_speed'}
RECT = ('shape', 'id', 'x', 'y', 'w', 'h', 'angle', 'material')
CIRCLE = ('shape', 'id', 'x', 'y', 'r', 'material')
JOINT = ('a', 'b', 'kind', 'x', 'y', 'motor_speed')


def canonical_design(d):
    """Normalized bytes for the well-formed designs these methods emit; None
    for anything outside that subset (the executor would decide it)."""
    try:
        bodies = []
        for b in d['bodies']:
            keys = RECT if b['shape'] == 'rect' else CIRCLE
            b = dict(b, angle=b.get('angle', 0.0)) if b['shape'] == 'rect' else b
            if set(b) != set(keys):
                return None
            bodies.append({k: b[k] for k in keys})
        joints = [{k: j.get(k) for k in JOINT} for j in d.get('joints', [])]
        out = {'language': d['language'], 'bodies': bodies, 'joints': joints, 'payload_on': d['payload_on']}
        if set(d) - set(out):
            return None
    except (KeyError, TypeError, AttributeError):
        return None
    return _enc(out, DESIGN_F32).encode()


def design_digest(d):
    b = canonical_design(d)
    return None if b is None else hashlib.sha256(b).hexdigest()
