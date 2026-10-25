"""pc1_specs.py writes receive specs exactly as process-comparison/1 declares.

Run with: python3 -m unittest discover -s experiments/invention-lab/scoring/tools
Only spec files are written and validated; nothing is executed.
"""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(f"{name}.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pc1 = load("pc1_specs")
receive = load("receive")
FAMILIES = ["ravine-crossing", "rough-transport", "return-mix", "projectile-target"]


class Pc1SpecTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="pc1-specs-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ("w1", "w2", "f1", "random", "parent"):
            (self.root / f"{name}.json").write_text(json.dumps({"package": {"id": name}}))

    def pinned(self, name):
        path = self.root / f"{name}.json"
        return {"package": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}

    def build(self, *argv):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            pc1.main([*argv, "--org", "org_b"])
        return out.getvalue().strip()

    def checked(self, out):
        return receive.load_spec(str(out / "spec.json"), str(out))[0]

    def test_dev_stage_is_one_candidate_on_seeds_0_to_5_in_family_order(self):
        out = self.root / "dev-w2"
        digest = self.build("dev", "--arm", "W", "--round", "2", "--candidate",
                            str(self.root / "w2.json"), "--control",
                            str(self.root / "random.json"), "--out", str(out))
        spec = self.checked(out)
        self.assertEqual(len(digest), 64)
        self.assertEqual((spec["instances"], spec["simulation_steps"], spec["wall_time_ms"],
                          spec["sandbox"]), (6, 72000, 60000, "namespaces"))
        self.assertEqual([t["family"] for t in spec["trials"]], FAMILIES)
        self.assertEqual(spec["trials"][0]["id"], "dev-w2-ravine-crossing")
        self.assertTrue(all("first_seed" not in t for t in spec["trials"]))
        self.assertTrue(all(t["control"] == "random-search" for t in spec["trials"]))
        with self.assertRaises(SystemExit):
            self.build("dev", "--arm", "X", "--round", "1", "--candidate", "a",
                       "--control", "b", "--out", str(self.root / "bad"))

    def test_fresh_stage_orders_plans_and_records_not_run_sides(self):
        sel = {fam: {"W": self.pinned("w1"), "F": self.pinned("f1")} for fam in FAMILIES}
        sel["return-mix"]["F"] = None
        (self.root / "sel.json").write_text(json.dumps(sel))
        out = self.root / "fresh"
        self.build("fresh", "--selections", str(self.root / "sel.json"), "--random",
                   str(self.root / "random.json"), "--parent", str(self.root / "parent.json"),
                   "--out", str(out))
        spec = self.checked(out)
        self.assertEqual(spec["instances"], 12)
        self.assertTrue(all(t["first_seed"] == 1000 for t in spec["trials"]))
        ids = [t["id"] for t in spec["trials"]]
        self.assertEqual(ids[:4], ["fresh-ravine-crossing-primary", "fresh-ravine-crossing-w-random",
                                   "fresh-ravine-crossing-f-random",
                                   "fresh-ravine-crossing-parent-random"])
        self.assertEqual(len(ids), 14)
        not_run = json.loads((out / "not-run.json").read_text())
        self.assertEqual([n["trial"] for n in not_run],
                         ["fresh-return-mix-primary", "fresh-return-mix-f-random"])
        # The parent baseline never depends on a selection.
        self.assertIn("fresh-return-mix-parent-random", ids)
        primary = spec["trials"][0]
        self.assertEqual((primary["candidate"], primary["control"]),
                         ("w-ravine-crossing", "f-ravine-crossing"))

    def test_outputs_are_create_only(self):
        args = ("dev", "--arm", "F", "--round", "1", "--candidate", str(self.root / "f1.json"),
                "--control", str(self.root / "random.json"), "--out", str(self.root / "d"))
        self.build(*args)
        with self.assertRaises(SystemExit):
            self.build(*args)

    def test_a_selection_must_pin_the_bytes_it_selected(self):
        sel = {fam: {"W": self.pinned("w1"), "F": None} for fam in FAMILIES}
        args = ("fresh", "--selections", str(self.root / "sel.json"), "--random",
                str(self.root / "random.json"), "--parent", str(self.root / "parent.json"))
        for name, change in (
            ("path only", lambda s: s["ravine-crossing"].update(W=str(self.root / "w1.json"))),
            ("extra key", lambda s: s["ravine-crossing"]["W"].update(round=1)),
            ("short digest", lambda s: s["ravine-crossing"]["W"].update(sha256="ab")),
            ("swapped bytes", lambda s: s["ravine-crossing"]["W"].update(
                package=str(self.root / "w2.json"))),
        ):
            with self.subTest(name):
                changed = json.loads(json.dumps(sel))
                change(changed)
                (self.root / "sel.json").write_text(json.dumps(changed))
                out = self.root / f"fresh-{name.replace(' ', '-')}"
                with self.assertRaises(SystemExit):
                    self.build(*args, "--out", str(out))
                self.assertFalse(out.exists())
        # A pinned random or parent package must match too.
        (self.root / "sel.json").write_text(json.dumps(sel))
        with self.assertRaises(SystemExit):
            self.build(*args, "--parent-sha256", "0" * 64, "--out", str(self.root / "p"))
        self.assertFalse((self.root / "p").exists())
        digest = hashlib.sha256((self.root / "parent.json").read_bytes()).hexdigest()
        self.build(*args, "--parent-sha256", digest, "--out", str(self.root / "ok"))
        self.assertTrue((self.root / "ok" / "spec.json").exists())
        self.assertEqual(
            sorted(p.name for p in self.root.iterdir() if p.name.startswith(".pc1-specs-")), [],
            "no staging directory is left behind")


if __name__ == "__main__":
    unittest.main()
