"""Generated-source acceptance for the hand split of generated method 8215c367.

These tests EXECUTE source: the retained model-generated parent and B's
generated child (both unpacked from method packages on main) and the split.
They are not part of unittest discovery. Run them explicitly from the
repository root, naming the execution boundary (see replay.py):

  python3 experiments/strategy-adapter/harness/acceptance_split.py --isolation namespaces

`namespaces` is lab-exec's sandbox boundary and needs root. `host` runs the
generated source directly on this machine with no isolation; use it only after
reading that source. Standard library only; no simulator, no model call. Each
run is bounded by replay.run_method's deadline and caps.
"""
import ast
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE))
import labworld  # noqa: E402
import replay  # noqa: E402

ISOLATION = None  # set from --isolation in __main__

EV = ROOT / 'docs/invention-lab/evidence'
VS_PARENT = EV / 'live-method-20261024-b-revision-receiver/vs-parent'
SPLIT = HERE.parent / 'parent-split'


def unpack(package_json, into):
    into.mkdir()
    pkg = json.loads(package_json.read_text())['package']
    for f in pkg['files']:
        (into / f['path']).write_text(f['content'])
    return into


class SplitAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = Path(tempfile.mkdtemp())
        cls.parent = unpack(EV / 'live-method-20261024-generated/method-package.json', cls.tmp / 'parent')
        cls.child = unpack(EV / 'live-method-20261024-b-revision/method-package.json', cls.tmp / 'child')

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp)

    def replay(self, slot, method_dir):
        oracle = replay.Retained(VS_PARENT / 'plan.json', VS_PARENT / slot)
        _, code = replay.run_method(method_dir, oracle, ISOLATION)
        return oracle.summary(code)

    def test_harness_reproduces_both_retained_runs(self):
        p = self.replay('control', self.parent)
        self.assertTrue(p['identical'], p['divergence'])
        self.assertEqual((p['experiments'], p['steps']), (6, 5205))
        c = self.replay('candidate', self.child)
        self.assertTrue(c['identical'], c['divergence'])
        self.assertEqual((c['experiments'], c['steps']), (32, 39778))

    def test_split_reproduces_parent_run(self):
        s = self.replay('control', SPLIT)
        self.assertTrue(s['identical'], s['divergence'])
        self.assertEqual((s['experiments'], s['steps']), (6, 5205))
        self.assertEqual(s['submitted'], '2483f08f8ec33f4144a4a6105842bd9e2da7702275592140c3aca35c2ac61cec')
        self.assertTrue(all(r['success'] for r in s['rows']))

    def test_a_changed_strategy_is_detected(self):
        bad = self.tmp / 'bad'
        shutil.copytree(SPLIT, bad)
        src = (bad / 'strategy.py').read_text().replace('reverse=True', 'reverse=False')
        src = src.replace('return sorted(items', 'return list(reversed(items)) or sorted(items')
        (bad / 'strategy.py').write_text(src)
        self.assertFalse(self.replay('control', bad)['identical'])

    def test_split_equals_parent_on_synthetic_cases(self):
        self.assertEqual(replay.diff(self.parent, SPLIT, 200, ISOLATION), {'identical': True, 'cases': 200})


if __name__ == '__main__':
    args = sys.argv[1:]
    if args[:1] != ['--isolation'] or len(args) < 2 or args[1] not in replay.ISOLATIONS:
        sys.exit(__doc__)
    ISOLATION = args[1]
    print('isolation: %s%s' % (ISOLATION, ' (HOST EXECUTION: generated source runs unsandboxed)' if ISOLATION == 'host' else ''),
          file=sys.stderr)
    unittest.main(argv=[sys.argv[0]] + args[2:])
