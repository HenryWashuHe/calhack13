#!/usr/bin/env python3
"""Offline checks for a method against the executor's line protocol.

  replay.py transcript <plan.json> <run-dir> <method-dir>
      Serve the retained run in <run-dir> (transcript.json, artifacts/,
      run.json) to the method in <method-dir>: worlds are rebuilt and checked
      against the plan's digests, and each experiment request must match the
      retained record (instance id and normalized design sha256) in order.
      Replies are the retained result artifacts. No simulator runs.

  replay.py diff <method-dir-a> <method-dir-b> [cases]
      Run both methods against the same SYNTHETIC oracle (seeded outcomes,
      refusals, invalid/failed/unknown results, missing fields) and compare
      every request byte for byte. Synthetic outcomes are for equivalence
      only and are never scores.

Standard library only.
"""
import hashlib
import json
import os
import random
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import labworld  # noqa: E402


def run_method(method_dir, oracle, timeout=60):
    """Drive `python3 method.py` in method_dir; return (request lines, exit code)."""
    p = subprocess.Popen([sys.executable, 'method.py'], cwd=method_dir, stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
                         env={'PATH': os.environ.get('PATH', '/usr/bin:/bin')})
    lines = []
    while True:
        line = p.stdout.readline()
        if not line:
            break
        lines.append(line)
        try:
            req = json.loads(line)
        except ValueError:
            req = None
        reply = oracle.reply(req) if isinstance(req, dict) else {'ok': False, 'refused': 'request is not JSON'}
        try:
            p.stdin.write(json.dumps(reply) + '\n')
            p.stdin.flush()
        except BrokenPipeError:
            break
    p.stdin.close()
    code = p.wait(timeout=timeout)
    p.stdout.close()
    return lines, code


class Retained:
    def __init__(self, plan_path, run_dir):
        plan = json.loads(Path(plan_path).read_text())
        run_dir = Path(run_dir)
        self.items = []
        for ref in plan['development_instances']:
            fam, _, seed = ref['id'].split('/')
            world = labworld.instance(fam, int(seed))
            if labworld.instance_digest(world) != ref['sha256']:
                raise ValueError('rebuilt world does not match plan digest for ' + ref['id'])
            self.items.append({'id': ref['id'], 'max_steps': labworld.max_steps(fam), 'world': world})
        self.grant = plan['development_grant']['simulation_steps']
        self.records = json.loads((run_dir / 'transcript.json').read_text())['ledger']['records']
        self.artifacts = run_dir / 'artifacts'
        self.retained_submission = json.loads((run_dir / 'run.json').read_text())['submission']['sha256']
        self.k = 0
        self.spent = 0
        self.rows = []
        self.submitted = None
        self.divergence = None

    def _diverge(self, why):
        if self.divergence is None:
            self.divergence = '%s at experiment %d' % (why, self.k)
        return {'ok': False, 'refused': 'replay divergence'}

    def reply(self, req):
        op = req.get('op')
        if op == 'instances':
            return {'ok': True, 'instances': self.items, 'remaining_steps': self.grant - self.spent}
        if op == 'experiment':
            if self.divergence:
                return self._diverge('')
            if self.k >= len(self.records):
                return self._diverge('extra experiment')
            rec = self.records[self.k]
            digest = labworld.design_digest(req.get('design'))
            if req.get('instance') != rec['call']['instance']['id'] or digest != rec['call']['design']['sha256']:
                return self._diverge('different request (%s, %s)' % (req.get('instance'), digest))
            out = rec['settlement']['outcome']
            if out.get('kind') != 'returned':
                return self._diverge('unreplayable record kind ' + str(out.get('kind')))
            name = out['result']['sha256']
            raw = (self.artifacts / name).read_bytes()
            if hashlib.sha256(raw).hexdigest() != name:
                raise ValueError('artifact bytes do not match their name ' + name)
            result = json.loads(raw)
            steps = rec['settlement']['simulation_steps']['value']
            self.spent += steps
            self.rows.append({'instance': rec['call']['instance']['id'], 'design': digest[:8],
                              'steps': steps, 'success': bool((result.get('metrics') or {}).get('success'))})
            reply = dict(result, ok=True, index=self.k, remaining_steps=self.grant - self.spent)
            self.k += 1
            return reply
        if op == 'submit':
            if self.submitted is not None:
                return {'ok': False, 'refused': 'already submitted'}
            self.submitted = labworld.design_digest(req.get('design'))
            return {'ok': True, 'output': self.submitted}
        return {'ok': False, 'refused': 'unknown op'}

    def summary(self, code):
        if self.divergence is None and self.k < len(self.records):
            self.divergence = 'stopped after %d of %d retained experiments' % (self.k, len(self.records))
        identical = (self.divergence is None and code == 0 and self.submitted == self.retained_submission)
        return {'identical': identical, 'experiments': self.k, 'retained_experiments': len(self.records),
                'steps': self.spent, 'submitted': self.submitted, 'retained_submission': self.retained_submission,
                'divergence': self.divergence, 'exit_code': code, 'rows': self.rows}


class Synthetic:
    """Deterministic fake executor. SYNTHETIC: not a simulator, not a score."""

    def __init__(self, seed):
        rnd = random.Random(seed)
        self.seed = seed
        fam = rnd.choice(labworld.FAMILIES)
        seeds = rnd.sample(range(10000), rnd.choice([0, 1, 2, 3, 6, 8]))
        self.items = [{'id': '%s/1/%d' % (fam, s), 'max_steps': labworld.max_steps(fam),
                       'world': labworld.instance(fam, s)} for s in seeds]
        for it in self.items:
            if rnd.random() < 0.15:
                del it['world']['rules']['budget']
        self.remaining = rnd.choice([0, 2400, 5 * 2400, 12 * 2400, 72000, 200 * 2400])
        self.refuse_instances = rnd.random() < 0.05
        self.omit_remaining = rnd.random() < 0.1
        self.p = rnd.choice([0.0, 0.05, 0.2, 0.5, 0.9])
        self.calls = 0
        self.submitted = False

    def reply(self, req):
        self.calls += 1
        r = random.Random('%d|%d|%s' % (self.seed, self.calls, json.dumps(req, sort_keys=True)))
        op = req.get('op')
        if op == 'instances':
            if self.refuse_instances:
                return {'ok': False, 'refused': 'synthetic'}
            return {'ok': True, 'instances': self.items, 'remaining_steps': self.remaining}
        if op == 'experiment':
            item = next((i for i in self.items if i['id'] == req.get('instance')), None)
            if item is None or self.remaining < item['max_steps'] or r.random() < 0.03:
                return {'ok': False, 'refused': 'synthetic'}
            u = r.random()
            if u < 0.04:
                res = {'outcome': 'unknown', 'steps': None}
            elif u < 0.09:
                res = {'outcome': 'invalid', 'reason': 'synthetic', 'steps': 0}
            elif u < 0.12:
                res = {'outcome': 'failed', 'reason': 'synthetic', 'steps': r.randint(1, item['max_steps'])}
            else:
                ok = r.random() < self.p
                res = {'outcome': 'scored', 'steps': r.randint(1, item['max_steps']),
                       'metrics': {'success': ok, 'progress': 1.0 if ok else round(r.random(), 3)}}
            self.remaining -= res['steps'] or 0
            res['ok'] = True
            if not self.omit_remaining:
                res['remaining_steps'] = self.remaining
            return res
        if op == 'submit':
            if self.submitted:
                return {'ok': False, 'refused': 'already submitted'}
            self.submitted = True
            return {'ok': True, 'output': 'synthetic'}
        return {'ok': False, 'refused': 'unknown op'}


def diff(a, b, cases):
    for seed in range(cases):
        ra = run_method(a, Synthetic(seed))
        rb = run_method(b, Synthetic(seed))
        if ra != rb:
            n = next((i for i, (x, y) in enumerate(zip(ra[0], rb[0])) if x != y), min(len(ra[0]), len(rb[0])))
            return {'identical': False, 'case': seed, 'first_difference_at_request': n}
    return {'identical': True, 'cases': cases}


def main(argv):
    if len(argv) == 5 and argv[1] == 'transcript':
        oracle = Retained(argv[2], argv[3])
        _, code = run_method(argv[4], oracle)
        out = oracle.summary(code)
    elif len(argv) in (4, 5) and argv[1] == 'diff':
        out = diff(argv[2], argv[3], int(argv[4]) if len(argv) == 5 else 300)
    else:
        print(__doc__, file=sys.stderr)
        return 2
    print(json.dumps(out, indent=1))
    return 0 if out['identical'] else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv))
