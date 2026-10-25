#!/usr/bin/env python3
"""Offline checks for a method against the executor's line protocol.

Starting a method executes its source. Every command that does so names the
execution boundary with --isolation; there is no default:

  namespaces  the boundary lab-exec uses for `namespaces` runs, copied from
              experiments/invention-lab/executor/src/sandbox.rs (needs root and
              util-linux unshare): no network, every mount read-only where the
              kernel allows it, private /home /root /tmp /var/tmp /dev/shm /run,
              the method runs as nobody with no_new_privs under prlimit. At the
              end of every run the namespace's first process is SIGKILLed and
              unshare's exit, which follows the end of the whole namespace, is
              awaited; if it is not observed the run raises. Not enforced: reads of the
              rest of the read-only system, a syscall filter, filesystem Unix
              sockets. It is not proof that no model was used.
  host        `python3 method.py` directly on this host with only PATH set. No
              isolation: the method can read and write what this user can and
              reach the network. Only for source you trust, such as hand-written
              fixtures; never for generated source you have not read.

Every run is bounded. One absolute deadline from process start covers every
read and write; a request line may not exceed 1 MiB (the executor's protocol
limit), checked again before each buffered request is answered (a slow oracle
reply is not interrupted, but no later request is dispatched); request count and
total output are capped. On every exit path, normal or not, the namespace is
ended as above, the process group is killed and reaped, and the namespaces stage
is removed, including when staging itself or termination fails. A host-mode
descendant that leaves the process group is not reached.

  replay.py transcript --isolation MODE <plan.json> <run-dir> <method-dir>
      Serve the retained run in <run-dir> (transcript.json, artifacts/,
      run.json) to the method in <method-dir>: worlds are rebuilt and checked
      against the plan's digests, and each experiment request must match the
      retained record (instance id and normalized design sha256) in order.
      Replies are the retained result artifacts. No simulator runs.

  replay.py diff --isolation MODE <method-dir-a> <method-dir-b> [cases]
      Run both methods against the same SYNTHETIC oracle (seeded outcomes,
      refusals, invalid/failed/unknown results, missing fields) and compare
      every request byte for byte. Synthetic outcomes are for equivalence
      only and are never scores.

Standard library only, plus `unshare` for namespaces.
"""
import hashlib
import json
import os
import random
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import labworld  # noqa: E402

ISOLATIONS = ('namespaces', 'host')
MAX_LINE = 1 << 20
MAX_REQUESTS = 4096
MAX_OUTPUT = 16 << 20
STAGE_ROOT = Path('/run/genesis-lab-replay')

# Copy of the setup in experiments/invention-lab/executor/src/sandbox.rs
# (`Mode::Namespaces`); the executor stays authoritative. Run as root inside
# fresh network, mount and PID namespaces before dropping to nobody.
NAMESPACES_SETUP = r"""set -e
mount --bind / /
awk '{print $5}' /proc/self/mountinfo | sort -u | while read -r m; do
  mount -o remount,bind,ro "$m" 2>/dev/null || true
done
mount -t tmpfs -o size=64m,mode=755 none /home
mount -t tmpfs -o size=1m,mode=700 none /root
mount -t tmpfs -o size=256m,mode=1777 none /tmp
mount -t tmpfs -o size=16m,mode=1777 none /var/tmp
mount -t tmpfs -o size=16m,mode=1777 none /dev/shm
mkdir -p /tmp/work
cp -R "$LAB_STAGE/files/." /tmp/work/
chmod -R a+rX /tmp/work
mount -t tmpfs -o size=1m,mode=755 none /run
cd /tmp/work
exec prlimit --as=1073741824 --nproc=64 --fsize=67108864 --nofile=256 -- \
  setpriv --reuid 65534 --regid 65534 --clear-groups --no-new-privs -- "$LAB_INTERPRETER" "/tmp/work/$LAB_ENTRY"
"""


class Stopped(RuntimeError):
    """A run hit its deadline or a bound; its process group was killed and reaped."""


def _launch(method_dir, isolation):
    """(argv, cwd, env, stage) for one run of method.py under `isolation`."""
    method_dir = Path(method_dir).resolve()
    if isolation == 'host':
        return [sys.executable, 'method.py'], str(method_dir), {'PATH': os.environ.get('PATH', '/usr/bin:/bin')}, None
    if isolation == 'namespaces':
        STAGE_ROOT.mkdir(mode=0o700, parents=True, exist_ok=True)
        stage = Path(tempfile.mkdtemp(prefix='run-', dir=STAGE_ROOT))
        try:
            shutil.copytree(method_dir, stage / 'files', ignore=shutil.ignore_patterns('__pycache__'))
        except BaseException:
            shutil.rmtree(stage, ignore_errors=True)
            raise
        argv = ['unshare', '--net', '--mount', '--pid', '--fork', '--mount-proc', '--propagation', 'private',
                '--kill-child=SIGKILL', '--', 'sh', '-c', NAMESPACES_SETUP]
        env = {'PATH': '/usr/local/bin:/usr/bin:/bin', 'LAB_STAGE': str(stage), 'LAB_ENTRY': 'method.py',
               'LAB_INTERPRETER': 'python3'}
        return argv, '/', env, stage
    raise ValueError('isolation must be one of %s, not %r' % (', '.join(ISOLATIONS), isolation))


def _namespace_inits(pid):
    """unshare's children: the PID namespace's first process. Same lookup as
    executor/src/sandbox.rs `namespace_inits`, with a /proc scan fallback."""
    try:
        return [int(x) for x in Path('/proc/%d/task/%d/children' % (pid, pid)).read_text().split() if int(x) > 0]
    except (OSError, ValueError):
        pass
    found = []
    for d in Path('/proc').iterdir():
        if d.name.isdigit():
            try:
                ppid = next(int(l.split()[1]) for l in (d / 'status').read_text().splitlines() if l.startswith('PPid:'))
            except (OSError, ValueError, IndexError, StopIteration):
                continue
            if ppid == pid:
                found.append(int(d.name))
    return found


def _in_other_pid_namespace(pid):
    try:
        return os.readlink('/proc/%d/ns/pid' % pid) != os.readlink('/proc/self/ns/pid')
    except OSError:
        return False


def _end_namespace(p, wait=10):
    """SIGKILL the namespace's first process, which ends every process in it,
    then wait for unshare: it exits only after its child, and the kernel lets
    that first process exit only after the rest of the namespace is gone. So
    unshare's exit is the observed end of the whole tree. Killing unshare or
    its process group instead would not do this: a method can leave the group,
    and the setpriv credential change clears unshare's --kill-child signal.
    Returns whether unshare's exit was observed within `wait` seconds."""
    deadline = time.monotonic() + wait
    while p.poll() is None:
        for pid in _namespace_inits(p.pid):
            # Only a process in another PID namespace is a namespace's first process.
            if _in_other_pid_namespace(pid):
                try:
                    os.kill(pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError):
                    pass
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.005)
    return True


def _stop(p):
    """Close the pipes, kill the whole process group and reap the leader."""
    for f in (p.stdin, p.stdout):
        try:
            f.close()
        except OSError:
            pass
    try:
        os.killpg(p.pid, signal.SIGKILL)  # the group outlives a leader that already exited
    except (ProcessLookupError, PermissionError):
        pass
    try:
        p.wait(timeout=10)
    except subprocess.TimeoutExpired:
        raise Stopped('process %d did not exit after SIGKILL' % p.pid)


def run_method(method_dir, oracle, isolation, timeout=60, max_line=MAX_LINE, max_requests=MAX_REQUESTS,
               max_output=MAX_OUTPUT):
    """Drive method.py in method_dir under `isolation`; return (request lines, exit code).

    Raises Stopped when the deadline or a bound is hit, after cleanup."""
    argv, cwd, env, stage = _launch(method_dir, isolation)
    deadline = time.monotonic() + timeout
    p = None
    lines = []

    def answer(line):
        if len(line) > max_line:
            raise Stopped('request line over %d bytes' % max_line)
        if len(lines) >= max_requests:
            raise Stopped('more than %d requests' % max_requests)
        text = line.decode('utf-8', 'replace')
        lines.append(text)
        try:
            req = json.loads(text)
        except ValueError:
            req = None
        reply = oracle.reply(req) if isinstance(req, dict) else {'ok': False, 'refused': 'request is not JSON'}
        return (json.dumps(reply) + '\n').encode()

    try:
        p = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, start_new_session=True)
        out, inp = p.stdout.fileno(), p.stdin.fileno()
        os.set_blocking(out, False)
        os.set_blocking(inp, False)
        buf, pending, total, writable = b'', b'', 0, True
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise Stopped('no end of output within %s s' % timeout)
            r, w, _ = select.select([out], [inp] if pending and writable else [], [], left)
            if w:
                try:
                    pending = pending[os.write(inp, pending):]
                except BlockingIOError:
                    pass
                except BrokenPipeError:
                    writable, pending = False, b''
            if not r:
                continue
            try:
                chunk = os.read(out, 1 << 16)
            except BlockingIOError:
                continue
            if not chunk:
                break
            total += len(chunk)
            if total > max_output:
                raise Stopped('more than %d bytes of output' % max_output)
            buf += chunk
            while b'\n' in buf:
                if time.monotonic() >= deadline:
                    raise Stopped('no end of output within %s s' % timeout)
                line, buf = buf.split(b'\n', 1)
                pending += answer(line + b'\n')
            if len(buf) > max_line:
                raise Stopped('request line over %d bytes' % max_line)
        if buf:
            if time.monotonic() >= deadline:
                raise Stopped('no end of output within %s s' % timeout)
            answer(buf)  # a last request without a newline; nobody reads the reply
        p.stdin.close()
        while p.poll() is None:
            if time.monotonic() >= deadline:
                raise Stopped('no exit within %s s' % timeout)
            time.sleep(0.005)
        return lines, p.returncode
    finally:
        ended = True
        try:
            if p is not None:
                try:
                    if isolation == 'namespaces':
                        ended = _end_namespace(p)
                finally:
                    _stop(p)
        finally:
            if stage is not None:
                shutil.rmtree(stage, ignore_errors=True)
        if not ended:
            raise Stopped('the PID namespace of process %d was not observed to end' % p.pid)


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


def diff(a, b, cases, isolation):
    for seed in range(cases):
        ra = run_method(a, Synthetic(seed), isolation)
        rb = run_method(b, Synthetic(seed), isolation)
        if ra != rb:
            n = next((i for i, (x, y) in enumerate(zip(ra[0], rb[0])) if x != y), min(len(ra[0]), len(rb[0])))
            return {'identical': False, 'case': seed, 'first_difference_at_request': n}
    return {'identical': True, 'cases': cases}


def main(argv):
    args = list(argv[1:])
    isolation = None
    if '--isolation' in args:
        i = args.index('--isolation')
        isolation = args[i + 1] if i + 1 < len(args) else None
        del args[i:i + 2]
    if isolation not in ISOLATIONS:
        print(__doc__, file=sys.stderr)
        print('--isolation namespaces|host is required', file=sys.stderr)
        return 2
    try:
        if len(args) == 4 and args[0] == 'transcript':
            oracle = Retained(args[1], args[2])
            _, code = run_method(args[3], oracle, isolation)
            out = oracle.summary(code)
        elif len(args) in (3, 4) and args[0] == 'diff':
            out = diff(args[1], args[2], int(args[3]) if len(args) == 4 else 300, isolation)
        else:
            print(__doc__, file=sys.stderr)
            return 2
    except Stopped as e:
        out = {'identical': False, 'stopped': str(e)}
    out['execution'] = {'isolation': isolation, 'host_execution': isolation == 'host'}
    print(json.dumps(out, indent=1))
    return 0 if out['identical'] else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv))
