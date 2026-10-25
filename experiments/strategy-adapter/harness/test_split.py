"""Acceptance tests for the hand split of generated method 8215c367.

Run from the repository root:  python3 -m unittest discover -s experiments/strategy-adapter/harness
Standard library only; no simulator, no model call.
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
        _, code = replay.run_method(method_dir, oracle)
        return oracle.summary(code)

    def test_worlds_rebuild_to_plan_digests(self):
        plan = json.loads((VS_PARENT / 'plan.json').read_text())
        for ref in plan['development_instances']:
            fam, _, seed = ref['id'].split('/')
            self.assertEqual(labworld.instance_digest(labworld.instance(fam, int(seed))), ref['sha256'])

    def test_rough_transport_has_no_ravine_features(self):
        # The parent's hardness key (gap + step_height) is 0.0 on every
        # instance here, so its "screen on the hardest" order is a no-op.
        for s in range(6):
            w = labworld.instance('rough-transport', s)
            self.assertEqual((w['gap'], w['step_height']), (0.0, 0.0))

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
        self.assertEqual(replay.diff(self.parent, SPLIT, 200), {'identical': True, 'cases': 200})

    def test_strategy_has_no_domain_nouns(self):
        banned = {'design', 'world', 'wheel', 'deck', 'budget', 'motor', 'instance', 'gap', 'cart',
                  'json', 'sys', 'send', 'experiment', 'submit'}
        tree = ast.parse((SPLIT / 'strategy.py').read_text())
        words = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Name):
                words.add(node.id.lower())
            elif isinstance(node, (ast.FunctionDef, ast.arg)):
                words.add((node.name if isinstance(node, ast.FunctionDef) else node.arg).lower())
            elif isinstance(node, ast.Constant) and isinstance(node.value, str) and node is not tree.body[0].value:
                words.add(node.value.lower())
            elif isinstance(node, (ast.Import, ast.ImportFrom)):
                words.update(a.name for a in node.names)
        self.assertEqual({w for w in words if any(b in w for b in banned)}, set())

    def test_adapter_has_no_search_loop(self):
        tree = ast.parse((SPLIT / 'adapter.py').read_text())
        for loop in (n for n in ast.walk(tree) if isinstance(n, (ast.For, ast.While))):
            calls = {getattr(c.func, 'id', getattr(c.func, 'attr', None))
                     for c in ast.walk(loop) if isinstance(c, ast.Call)}
            self.assertFalse(calls & {'send', 'evaluate'}, 'adapter loop issues requests')


if __name__ == '__main__':
    unittest.main()
