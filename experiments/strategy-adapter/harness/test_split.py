"""Static checks for the hand split of generated method 8215c367.

Run from the repository root:  python3 -m unittest discover -s experiments/strategy-adapter/harness
Standard library only. Nothing here starts a method: the checks that execute
the retained generated parent and child, and the split, are in
acceptance_split.py, which must be run explicitly with a named isolation.
"""
import ast
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE))
import labworld  # noqa: E402

EV = ROOT / 'docs/invention-lab/evidence'
VS_PARENT = EV / 'live-method-20261024-b-revision-receiver/vs-parent'
SPLIT = HERE.parent / 'parent-split'


class SplitStatic(unittest.TestCase):
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
