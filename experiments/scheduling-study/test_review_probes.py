"""Review probes for PR #201 (scheduling study).

Each test asserts a guarantee stated by the protocol or the code's docstrings.
A failing test is a candidate defect, not a harness expectation. Run from this
directory with ``python3 -m unittest test_review_probes -v``.
"""

import ast
import copy
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import benchmark
import run_local_pilot
from benchmark import (STRATA, canonical_bytes, digest, generate_instance, make_policy,
                       schedule)
from independent_checker import check_schedule
from run_local_pilot import run_pilot
from verify_run import verify

HERE = Path(__file__).resolve().parent


# ---------------------------------------------------------------- helpers

def read_lines(run: Path) -> list[dict]:
    return [json.loads(line) for line in (run / "events.jsonl").read_text("ascii").splitlines()]


def write_rechained(run: Path, records: list[dict]) -> None:
    """Rewrite events.jsonl with a valid chain over the given records."""
    previous = "0" * 64
    out = []
    for sequence, record in enumerate(records):
        body = {"sequence": sequence, "previous_sha256": previous, "record": record}
        current = hashlib.sha256(canonical_bytes(body)).hexdigest()
        out.append(canonical_bytes({**body, "sha256": current}) + b"\n")
        previous = current
    (run / "events.jsonl").write_bytes(b"".join(out))


def write_json(path: Path, value) -> None:
    path.write_bytes(canonical_bytes(value) + b"\n")


def rebind(run: Path, records: list[dict], manifest=None, summary=None) -> None:
    """Re-derive manifest/summary hashes in summary and journal, then re-chain."""
    manifest = manifest if manifest is not None else json.loads((run / "manifest.json").read_text())
    summary = summary if summary is not None else json.loads((run / "summary.json").read_text())
    summary = {**summary, "manifest_sha256": digest(manifest)}
    write_json(run / "manifest.json", manifest)
    write_json(run / "summary.json", summary)
    for record in records:
        if record["event"] == "run_started":
            record["manifest_sha256"] = digest(manifest)
        if record["event"] == "run_completed":
            record["summary_sha256"] = digest(summary)
    write_rechained(run, records)


class _Patched:
    """Temporarily replace attributes on a module."""

    def __init__(self, module, **attrs):
        self.module, self.attrs, self.saved = module, attrs, {}

    def __enter__(self):
        for name, value in self.attrs.items():
            self.saved[name] = getattr(self.module, name)
            setattr(self.module, name, value)

    def __exit__(self, *exc):
        for name, value in self.saved.items():
            setattr(self.module, name, value)


# ---------------------------------------------------------------- checker

class CheckerProbes(unittest.TestCase):
    """independent_checker.check_schedule must reject every invalid schedule."""

    @classmethod
    def setUpClass(cls):
        cls.instance = generate_instance(81000, "balanced")
        cls.entries = schedule(cls.instance, make_policy((1, 0, 0, 0, 0, 0)))
        cls.makespan = check_schedule(cls.instance, cls.entries)

    def bad(self):
        return copy.deepcopy(self.entries)

    def assertRejected(self, entries, instance=None):
        with self.assertRaises((ValueError, TypeError, KeyError)):
            check_schedule(instance if instance is not None else self.instance, entries)

    def test_checker_does_not_import_benchmark(self):
        tree = ast.parse((HERE / "independent_checker.py").read_text())
        imported = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imported |= {alias.name for alias in node.names}
            elif isinstance(node, ast.ImportFrom):
                imported.add(node.module)
        self.assertEqual(imported - {"__future__", "typing"}, set())

    def test_rejects_ineligible_machine(self):
        bad = self.bad()
        entry = bad[0]
        allowed = {c["machine"] for c in self.instance["jobs"][entry["job"]][entry["operation"]]}
        entry["machine"] = next(m for m in range(12) if m not in allowed)
        self.assertRejected(bad)

    def test_rejects_eligible_machine_with_other_machines_duration(self):
        for index, entry in enumerate(self.entries):
            alts = self.instance["jobs"][entry["job"]][entry["operation"]]
            durations = {c["machine"]: c["duration"] for c in alts}
            other = next((d for m, d in durations.items()
                          if d != durations[entry["machine"]]), None)
            if other is not None:
                bad = self.bad()
                bad[index]["duration"] = other
                self.assertRejected(bad)
                return
        self.fail("no operation with distinct alternative durations")

    def test_rejects_duration_off_by_one(self):
        for delta in (-1, 1):
            bad = self.bad()
            bad[5]["duration"] += delta
            with self.subTest(delta=delta):
                self.assertRejected(bad)

    def test_rejects_start_before_predecessor_end(self):
        for index, entry in enumerate(self.entries):
            if entry["operation"] == 0:
                continue
            prev = next(e for e in self.entries
                        if e["job"] == entry["job"] and e["operation"] == entry["operation"] - 1)
            bad = self.bad()
            bad[index]["start"] = prev["start"] + prev["duration"] - 1
            if bad[index]["start"] < 0:
                continue
            self.assertRejected(bad)
            return

    def test_accepts_touching_job_and_machine_edges(self):
        # Touching edges are legal; the produced schedule already contains some.
        ends = {(e["machine"], e["start"] + e["duration"]) for e in self.entries}
        touching = sum((e["machine"], e["start"]) in ends for e in self.entries)
        self.assertGreater(touching, 0)
        self.assertEqual(check_schedule(self.instance, self.entries), self.makespan)

    def test_rejects_one_unit_machine_overlap_non_adjacent(self):
        by_machine = {}
        for index, entry in enumerate(self.entries):
            by_machine.setdefault(entry["machine"], []).append(index)
        indices = sorted(by_machine[self.entries[0]["machine"]],
                         key=lambda i: self.entries[i]["start"])
        # Stretch the first interval over the third one by moving the third
        # earlier into the first's span (keeps precedence possibly broken too,
        # so check the overlap message specifically on a safe move).
        bad = self.bad()
        first, second = bad[indices[0]], bad[indices[1]]
        second["start"] = first["start"] + first["duration"] - 1
        self.assertRejected(bad)

    def test_rejects_identical_interval_on_same_machine(self):
        bad = self.bad()
        machine = bad[0]["machine"]
        other = next(e for e in bad[1:] if e["machine"] == machine)
        # Choose a same-duration op if possible; otherwise just align starts.
        other["start"] = bad[0]["start"]
        self.assertRejected(bad)

    def test_rejects_malformed_numbers(self):
        for value in (-1, 1.0, float("nan"), float("inf"), float("-inf"), True, False,
                      "0", None, 10**12):
            for field in ("start", "duration", "job", "operation", "machine"):
                bad = self.bad()
                bad[7][field] = value
                with self.subTest(field=field, value=value):
                    self.assertRejected(bad)

    def test_rejects_zero_and_negative_duration(self):
        for value in (0, -3):
            bad = self.bad()
            bad[9]["duration"] = value
            with self.subTest(value=value):
                self.assertRejected(bad)

    def test_rejects_duplicate_missing_extra_and_unknown_operations(self):
        variants = {
            "missing": self.entries[1:],
            "duplicate_replacing": [self.entries[1]] + self.entries[1:],
            "extra": self.entries + [self.entries[0]],
            "unknown_job": [dict(self.entries[0], job=60)] + self.entries[1:],
            "unknown_op": [dict(self.entries[0], operation=6)] + self.entries[1:],
            "extra_field": [dict(self.entries[0], note="x")] + self.entries[1:],
            "missing_field": [{k: v for k, v in self.entries[0].items() if k != "start"}]
                             + self.entries[1:],
            "not_list": tuple(self.entries),
            "entry_not_dict": [list(self.entries[0].values())] + self.entries[1:],
        }
        for name, bad in variants.items():
            with self.subTest(name=name):
                self.assertRejected(bad)

    def test_rejects_time_limit(self):
        bad = self.bad()
        bad[0]["start"] = 1_000_000
        self.assertRejected(bad)

    def test_makespan_equals_max_end(self):
        self.assertEqual(self.makespan, max(e["start"] + e["duration"] for e in self.entries))

    def test_rejects_malformed_instance(self):
        mutations = []
        inst = copy.deepcopy(self.instance)
        inst["machine_count"] = True
        mutations.append(inst)
        inst = copy.deepcopy(self.instance)
        inst["jobs"] = inst["jobs"][:59]
        mutations.append(inst)
        inst = copy.deepcopy(self.instance)
        e = self.entries[0]
        inst["jobs"][e["job"]][e["operation"]].append(
            dict(inst["jobs"][e["job"]][e["operation"]][0]))  # duplicate machine
        mutations.append(inst)
        for index, inst in enumerate(mutations):
            with self.subTest(index=index):
                self.assertRejected(self.entries, inst)


# ---------------------------------------------------------------- generator / decoder

class GeneratorDecoderProbes(unittest.TestCase):
    def test_generator_declared_shape(self):
        for seed in range(81000, 81009):
            for stratum in STRATA:
                inst = generate_instance(seed, stratum)
                self.assertEqual(len(inst["jobs"]), 60)
                for job in inst["jobs"]:
                    self.assertEqual(len(job), 6)
                    for alts in job:
                        self.assertTrue(2 <= len(alts) <= 4)
                        machines = [a["machine"] for a in alts]
                        self.assertEqual(len(set(machines)), len(machines))
                        for a in alts:
                            self.assertTrue(1 <= a["duration"] <= 30)
                            self.assertTrue(0 <= a["machine"] < 12)
                            if stratum == "restricted":
                                self.assertLess(a["machine"], 6)
                            if stratum == "bottleneck":
                                lo, hi = (1, 8) if a["machine"] < 3 else (20, 30)
                                self.assertTrue(lo <= a["duration"] <= hi)

    def test_decoder_output_always_passes_checker(self):
        draw = benchmark.DeterministicDraw(424242, "balanced")
        for trial in range(9):
            weights = tuple(draw.below(17) - 8 for _ in range(6))
            if not any(weights):
                weights = (1, 0, 0, 0, 0, 0)
            inst = generate_instance(500 + trial, STRATA[trial % 3])
            entries = schedule(inst, make_policy(weights))
            with self.subTest(weights=weights):
                self.assertGreater(check_schedule(inst, entries), 0)

    def test_policy_rejects_non_int_weights(self):
        for value in (1.0, float("nan"), True, 9, -9, "1", None):
            weights = dict(zip(benchmark.FEATURES, (1, 0, 0, 0, 0, 0)))
            weights["duration"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                benchmark.validate_policy({"schema": "linear-dispatch/1", "weights": weights})

    def test_determinism_across_hash_seeds(self):
        code = ("import sys; sys.path.insert(0, %r)\n"
                "from benchmark import *\n"
                "print(digest([schedule(generate_instance(s, st), make_policy((1,3,-2,3,0,-1)))"
                " for s, st in ((1,'balanced'),(2,'bottleneck'),(3,'restricted'))]))" % str(HERE))
        outputs = set()
        for hash_seed in ("0", "1", "12345", "random"):
            env = dict(os.environ, PYTHONHASHSEED=hash_seed)
            outputs.add(subprocess.run([sys.executable, "-I", "-c", code], env=env,
                                       capture_output=True, text=True, check=True).stdout)
        self.assertEqual(len(outputs), 1, outputs)

    def test_development_and_calibration_seeds_disjoint_across_declared_runs(self):
        declared = [81000, 83000, 84000, 85000, 86000, 87000]
        development = {(s + i, STRATA[i % 3]) for s in declared for i in range(20)}
        calibration = {(s + 10_000 + i, STRATA[i % 3]) for s in declared for i in range(32)}
        self.assertEqual(development & calibration, set())
        # Instance content, not just seed labels, must be distinct too.
        dev_digests = {digest(generate_instance(*k)) for k in development}
        cal_digests = {digest(generate_instance(*k)) for k in calibration}
        self.assertEqual(dev_digests & cal_digests, set())


# ---------------------------------------------------------------- verifier

class VerifierProbes(unittest.TestCase):
    """verify_run.verify must reject runs that break the stated record guarantees."""

    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp())
        cls.base = cls.root / "base"
        run_pilot(cls.base, 82000, 1)
        verify(cls.base)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.root)

    def copy(self, name):
        target = self.root / name
        shutil.copytree(self.base, target)
        return target

    def assertVerifyRejects(self, run):
        with self.assertRaises((ValueError, KeyError, TypeError, IndexError)):
            verify(run)

    # --- tampering that the chain should catch (expected to hold)

    def test_rejects_tail_truncation(self):
        run = self.copy("truncate")
        lines = (run / "events.jsonl").read_bytes().splitlines(keepends=True)
        (run / "events.jsonl").write_bytes(b"".join(lines[:-1]))
        self.assertVerifyRejects(run)

    def test_rejects_reordered_events(self):
        run = self.copy("reorder")
        lines = (run / "events.jsonl").read_bytes().splitlines(keepends=True)
        lines[5], lines[6] = lines[6], lines[5]
        (run / "events.jsonl").write_bytes(b"".join(lines))
        self.assertVerifyRejects(run)

    def test_rejects_duplicated_event(self):
        run = self.copy("dup")
        lines = (run / "events.jsonl").read_bytes().splitlines(keepends=True)
        (run / "events.jsonl").write_bytes(b"".join(lines[:6] + [lines[5]] + lines[6:]))
        self.assertVerifyRejects(run)

    def test_rejects_rechained_duplicate_evaluation(self):
        run = self.copy("dup-rechained")
        records = [e["record"] for e in read_lines(run)]
        index = next(i for i, r in enumerate(records) if r["event"] == "policy_evaluated")
        write_rechained(run, records[:index + 1] + [records[index]] + records[index + 1:])
        self.assertVerifyRejects(run)

    def test_rejects_edited_field_without_rehash(self):
        run = self.copy("edit")
        lines = read_lines(run)
        index = next(i for i, e in enumerate(lines) if e["record"]["event"] == "policy_evaluated")
        lines[index]["record"]["elapsed_ns"] += 1
        (run / "events.jsonl").write_bytes(b"".join(canonical_bytes(e) + b"\n" for e in lines))
        self.assertVerifyRejects(run)

    def test_rejects_rechained_wrong_makespan(self):
        run = self.copy("makespan")
        records = [e["record"] for e in read_lines(run)]
        index = next(i for i, r in enumerate(records) if r["event"] == "policy_evaluated")
        records[index]["makespan"] -= 1
        write_rechained(run, records)
        self.assertVerifyRejects(run)

    def test_rejects_summary_mean_edit(self):
        run = self.copy("summary-mean")
        summary = json.loads((run / "summary.json").read_text())
        summary["calibration"]["earliest_finish"]["mean_makespan"] -= 1
        rebind(run, [e["record"] for e in read_lines(run)], summary=summary)
        self.assertVerifyRejects(run)

    # --- semantic guarantees (probes for defects)

    def test_rejects_manifest_without_source_hashes(self):
        """Manifest 'binds source hashes'; verify must not accept an empty binding."""
        run = self.copy("no-source-hashes")
        manifest = json.loads((run / "manifest.json").read_text())
        manifest["source_sha256"] = {}
        rebind(run, [e["record"] for e in read_lines(run)], manifest=manifest)
        self.assertVerifyRejects(run)

    def test_rejects_instances_not_generated_from_manifest_seed(self):
        """'A seed and stratum reproduce an instance': verify must bind instances to the seed."""
        run = self.root / "wrong-instances"
        real = run_local_pilot.generate_instance
        with _Patched(run_local_pilot,
                      generate_instance=lambda seed, stratum: real(seed + 777, stratum)):
            run_pilot(run, 82000, 0)
        self.assertVerifyRejects(run)

    def test_rejects_mislabeled_baseline_policy(self):
        """Reported 'Earliest finish' numbers must come from the earliest-finish policy."""
        run = self.root / "mislabeled-baseline"
        baselines = dict(run_local_pilot.BASELINES, earliest_finish=(1, 3, -2, 3, 0, -1))
        with _Patched(run_local_pilot, BASELINES=baselines):
            run_pilot(run, 82000, 0)
        self.assertVerifyRejects(run)

    def test_rejects_random_roster_not_derived_from_seed(self):
        """Random candidates are drawn from seed+1e6; a different roster is a hidden search."""
        run = self.root / "cherry-picked-random"
        real = run_local_pilot.DeterministicDraw
        with _Patched(run_local_pilot,
                      DeterministicDraw=lambda seed, stratum: real(seed + 99, stratum)):
            run_pilot(run, 82000, 1)
        self.assertVerifyRejects(run)

    def test_rejects_calibration_evaluation_of_unselected_candidate(self):
        """Selection is 'development only': no calibration peeking at other candidates."""
        run = self.copy("calibration-peek")
        records = [e["record"] for e in read_lines(run)]
        instances = json.loads((run / "instances.json").read_text())
        policies = {r["candidate"]: r["policy"] for r in records if r["event"] == "candidate_started"}
        summary = json.loads((run / "summary.json").read_text())
        selected = summary["selected_random_candidate"]
        # Add a second (worse) random candidate and evaluate it on calibration
        # before selection; it is not in the selected calibration set.
        extra_policy = make_policy((2, 0, 0, 0, 0, 1))
        name = "random_001"
        start = next(i for i, r in enumerate(records) if r["event"] == "candidate_selected")
        new = [{"event": "candidate_started", "timestamp_utc": "x", "candidate": name,
                "policy_sha256": digest(extra_policy), "policy": extra_policy}]
        dev_scores = []
        for idx, inst in enumerate(instances["development"]):
            entries = schedule(inst, extra_policy)
            score = check_schedule(inst, entries)
            dev_scores.append(score)
            new.append({"event": "policy_evaluated", "phase": "development", "candidate": name,
                        "instance_index": idx, "instance_sha256": digest(inst),
                        "schedule_sha256": digest(entries), "makespan": score,
                        "elapsed_ns": 1, "status": "feasible", "timestamp_utc": "x"})
        new.append({"event": "candidate_completed", "timestamp_utc": "x", "candidate": name,
                    "development_mean_makespan": sum(dev_scores) / len(dev_scores),
                    "development_count": len(dev_scores)})
        # Calibration peek at the extra candidate before selection.
        for idx, inst in enumerate(instances["calibration"]):
            entries = schedule(inst, extra_policy)
            new.append({"event": "policy_evaluated", "phase": "calibration", "candidate": name,
                        "instance_index": idx, "instance_sha256": digest(inst),
                        "schedule_sha256": digest(entries),
                        "makespan": check_schedule(inst, entries),
                        "elapsed_ns": 1, "status": "feasible", "timestamp_utc": "x"})
        records = records[:start] + new + records[start:]
        # Keep the selection consistent with development-only recomputation.
        random_dev = {}
        for r in records:
            if r["event"] == "candidate_completed" and r["candidate"].startswith("random_"):
                random_dev[r["candidate"]] = r["development_mean_makespan"]
        all_policies = dict(policies, **{name: extra_policy})
        best = min(random_dev, key=lambda n: (random_dev[n], digest(all_policies[n])))
        if best != selected:
            self.skipTest("extra candidate would change selection; pick another policy")
        manifest = json.loads((run / "manifest.json").read_text())
        manifest["random_candidates"] = 2
        summary["candidate_count"] += 1
        summary["development_evaluations"] += 20
        summary["calibration_evaluations"] += 32
        rebind(run, records, manifest=manifest, summary=summary)
        self.assertVerifyRejects(run)

    def test_rejects_summary_missing_a_stratum(self):
        """Per-stratum results (a declared success criterion) must be complete."""
        run = self.copy("missing-stratum")
        summary = json.loads((run / "summary.json").read_text())
        del summary["calibration"]["earliest_finish"]["by_stratum"]["bottleneck"]
        rebind(run, [e["record"] for e in read_lines(run)], summary=summary)
        self.assertVerifyRejects(run)

    def test_rejects_non_finite_number_spelled_as_overflow(self):
        """read_json claims to reject non-finite JSON; 1e999 parses to inf."""
        run = self.copy("overflow-inf")
        records = [e["record"] for e in read_lines(run)]
        text = (run / "summary.json").read_text()
        summary = json.loads(text)
        text = text.replace('"elapsed_ns":%d' % summary["elapsed_ns"], '"elapsed_ns":1e999')
        (run / "summary.json").write_text(text)
        parsed = json.loads(text)
        self.assertTrue(math.isinf(parsed["elapsed_ns"]))
        records[-1]["summary_sha256"] = digest(parsed)
        write_rechained(run, records)
        self.assertVerifyRejects(run)

    def test_rejects_non_canonical_or_duplicate_key_artifact_bytes(self):
        """Hashes are over parsed values; raw file bytes may say something else."""
        run = self.copy("duplicate-keys")
        text = (run / "summary.json").read_text()
        summary = json.loads(text)
        real = summary["calibration"]["earliest_finish"]["mean_makespan"]
        marker = '"earliest_finish":{'
        self.assertIn(marker, text)
        text = text.replace(marker, marker + '"mean_makespan":1.0,', 1)
        (run / "summary.json").write_text(text)
        self.assertEqual(json.loads(text)["calibration"]["earliest_finish"]["mean_makespan"], real)
        self.assertVerifyRejects(run)

    def test_rejects_duplicate_key_in_event_line(self):
        run = self.copy("duplicate-event-key")
        lines = (run / "events.jsonl").read_text().splitlines()
        index = next(i for i, l in enumerate(lines) if '"policy_evaluated"' in l)
        lines[index] = lines[index].replace('"record":{', '"record":{"makespan":1,', 1)
        (run / "events.jsonl").write_text("\n".join(lines) + "\n")
        self.assertVerifyRejects(run)

    def test_rejects_failure_or_unknown_event_inside_completed_run(self):
        run = self.copy("run-failed-inside")
        records = [e["record"] for e in read_lines(run)]
        records.insert(2, {"event": "run_failed", "timestamp_utc": "x",
                           "error_type": "RuntimeError", "error_message": "crash"})
        write_rechained(run, records)
        self.assertVerifyRejects(run)

    def test_returned_head_matches_last_line(self):
        run = self.copy("head")
        result = verify(run)
        self.assertEqual(result["last_event_sha256"], read_lines(run)[-1]["sha256"])


# ---------------------------------------------------------------- protocol text

class ProtocolTextProbes(unittest.TestCase):
    def test_published_hashes_are_full_sha256(self):
        import re
        text = (HERE.parent.parent / "docs/invention-lab/scheduling-study-protocol.md").read_text()
        hashes = re.findall(r"`([0-9a-f]{40,})`", text)
        self.assertGreaterEqual(len(hashes), 7)
        for value in hashes:
            with self.subTest(value=value):
                self.assertEqual(len(value), 64)


if __name__ == "__main__":
    unittest.main()
