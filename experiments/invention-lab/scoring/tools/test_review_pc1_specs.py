"""Independent preparation-only guards: inert files, no runner/provider calls."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

loader = importlib.util.spec_from_file_location(
    "review_pc1_specs", Path(__file__).with_name("pc1_specs.py"))
pc1 = importlib.util.module_from_spec(loader)
loader.loader.exec_module(pc1)


class BundlePublicationReview(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="pc1-independent-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.package = self.root / "inert.json"
        self.package.write_text('{"package":{"id":"inert"}}')
        self.selections = {
            family: {"W": str(self.package), "F": str(self.package)}
            for family in pc1.FAMILIES}
        self.sel = self.root / "selection.json"
        self.out = self.root / "prepared"

    def prepare(self):
        self.sel.write_text(json.dumps(self.selections))
        with contextlib.redirect_stdout(io.StringIO()), patch(
                "subprocess.run", side_effect=AssertionError("execution forbidden")):
            pc1.main(["fresh", "--selections", str(self.sel),
                      "--random", str(self.package), "--parent", str(self.package),
                      "--org", "org_receiver", "--out", str(self.out)])

    def test_sidecar_collision_must_not_publish_new_spec_before_failing(self):
        self.selections["return-mix"]["F"] = None
        self.out.mkdir()
        sidecar = self.out / "not-run.json"
        original = b'[{"status":"previous independent declaration"}]\n'
        sidecar.write_bytes(original)
        with self.assertRaises((FileExistsError, SystemExit)):
            self.prepare()
        self.assertEqual(sidecar.read_bytes(), original)
        self.assertFalse((self.out / "spec.json").exists(),
                         "failed pair publication left a new runnable spec")

    def test_all_present_selection_must_not_succeed_beside_stale_not_run(self):
        self.out.mkdir()
        (self.out / "not-run.json").write_text(json.dumps([{
            "trial": "fresh-ravine-crossing-primary", "family": "ravine-crossing",
            "status": "not_run", "reason": "no_candidate: previous declaration"}]))
        with self.assertRaises((FileExistsError, SystemExit)):
            self.prepare()
        self.assertFalse((self.out / "spec.json").exists())

    def test_all_absent_still_declares_all_four_parent_baselines_once(self):
        self.selections = {family: {"W": None, "F": None} for family in pc1.FAMILIES}
        self.prepare()
        spec = json.loads((self.out / "spec.json").read_text())
        missing = json.loads((self.out / "not-run.json").read_text())
        ran_ids = [trial["id"] for trial in spec["trials"]]
        missing_ids = [trial["trial"] for trial in missing]
        self.assertEqual(ran_ids, [f"fresh-{f}-parent-random" for f in pc1.FAMILIES])
        self.assertEqual(len(missing_ids), 12)
        self.assertEqual(len(set(ran_ids + missing_ids)), 16)
        self.assertFalse(set(ran_ids).intersection(missing_ids))
        self.assertTrue(all(trial["first_seed"] == 1000 for trial in spec["trials"]))

    def test_missing_extra_family_or_arm_cannot_shrink_the_declared_grid(self):
        for mode in ("missing_family", "extra_family", "missing_arm"):
            with self.subTest(mode=mode):
                selections = json.loads(json.dumps(self.selections))
                if mode == "missing_family":
                    selections.pop("return-mix")
                elif mode == "extra_family":
                    selections["other-family"] = {"W": None, "F": None}
                else:
                    selections["return-mix"].pop("W")
                with self.assertRaises(SystemExit):
                    pc1.fresh_spec(str(self.out), selections, str(self.package),
                                   str(self.package), "org_receiver")


if __name__ == "__main__":
    unittest.main()
