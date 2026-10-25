"""Verify an offline pilot's complete journal, artifacts and scored schedules."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
from typing import Any

from benchmark import (DeterministicDraw, STRATA, canonical_bytes, digest,
                       generate_instance, make_policy, schedule)
from independent_checker import check_schedule


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def finite_float(value: str) -> float:
    number = float(value)
    if not math.isfinite(number):
        raise ValueError(f"non-finite JSON number: {value}")
    return number


def strict_json(raw: bytes) -> Any:
    value = json.loads(raw.decode("ascii"), object_pairs_hook=reject_duplicate_keys,
                       parse_float=finite_float,
                       parse_constant=lambda value: (_ for _ in ()).throw(
                           ValueError(f"non-finite JSON constant: {value}")))
    require(raw == canonical_bytes(value) + b"\n", "non-canonical JSON bytes")
    return value


def read_json(path: Path) -> Any:
    return strict_json(path.read_bytes())


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


FIXED_POLICIES = {
    "earliest_finish": (1, 0, 0, 0, 0, 0),
    "shortest_duration": (0, 1, 0, 0, 0, 0),
    "remaining_work_first": (0, 0, -1, 0, 0, 0),
    "least_loaded_machine": (0, 0, 0, 1, 0, 0),
}


def expected_policies(seed: int, random_count: int) -> dict[str, dict[str, Any]]:
    expected = {name: make_policy(weights) for name, weights in FIXED_POLICIES.items()}
    draw = DeterministicDraw(seed + 1_000_000, "balanced")
    for index in range(random_count):
        weights = tuple(draw.below(7) - 3 for _ in range(6))
        if not any(weights):
            weights = (1, 0, 0, 0, 0, 0)
        expected[f"random_{index:03d}"] = make_policy(weights)
    return expected


def verify_event_order(records: list[dict[str, Any]], candidates: list[str],
                       selected: str | None, development_count: int,
                       calibration_count: int) -> None:
    """Check the v1 producer's declared sequence, including the selection barrier."""
    cursor = 0

    def expect(event: str, **fields: Any) -> None:
        nonlocal cursor
        require(cursor < len(records), f"missing {event} event")
        record = records[cursor]
        require(record.get("event") == event, f"unexpected event at position {cursor}")
        for name, value in fields.items():
            require(record.get(name) == value, f"{event} {name} mismatch")
        cursor += 1

    expect("run_started")
    expect("instances_frozen")
    for name in candidates:
        expect("candidate_started", candidate=name)
        for index in range(development_count):
            expect("policy_evaluated", phase="development", candidate=name,
                   instance_index=index)
        expect("candidate_completed", candidate=name)
    if selected is not None:
        expect("candidate_selected", candidate=selected, basis="development_only")
    for name in list(FIXED_POLICIES) + ([selected] if selected is not None else []):
        for index in range(calibration_count):
            expect("policy_evaluated", phase="calibration", candidate=name,
                   instance_index=index)
    expect("run_completed")
    require(cursor == len(records), "extra event after completion")


def verify(output_dir: Path, expected_head: str | None = None) -> dict[str, Any]:
    if expected_head is not None:
        require(re.fullmatch(r"[0-9a-f]{64}", expected_head) is not None,
                "expected head must be a lowercase SHA-256 hex digest")
    manifest = read_json(output_dir / "manifest.json")
    instances = read_json(output_dir / "instances.json")
    summary = read_json(output_dir / "summary.json")
    require(manifest["schema"] == "scheduling-study-offline-pilot/1", "wrong manifest schema")
    require(manifest["formal_test"] is False and summary["formal_test"] is False,
            "offline pilot must not claim formal evidence")
    require(manifest["model_requests"] == 0 and manifest["provider_requests"] == 0,
            "offline pilot must have zero external requests")
    require(manifest["stage"] == "offline_calibration" and
            summary["stage"] == "offline_calibration", "wrong study stage")
    require(manifest["development_instances"] == 20 and
            manifest["calibration_instances"] == 32 and
            manifest["strata"] == list(STRATA), "unsupported instance roster declaration")
    seed = manifest["seed"]
    random_count = manifest["random_candidates"]
    require(type(seed) is int and 0 <= seed <= 2**64 - 1_000_001,
            "invalid declared seed")
    require(type(random_count) is int and 0 <= random_count <= 200,
            "invalid declared random candidate count")
    require(set(instances) == {"development", "calibration"},
            "unexpected instance bundle fields")
    require(len(instances["development"]) == manifest["development_instances"],
            "development instance count mismatch")
    require(len(instances["calibration"]) == manifest["calibration_instances"],
            "calibration instance count mismatch")
    for index, actual in enumerate(instances["development"]):
        require(actual == generate_instance(seed + index, STRATA[index % 3]),
                f"development instance {index} differs from declared seed")
    for index, actual in enumerate(instances["calibration"]):
        require(actual == generate_instance(seed + 10_000 + index, STRATA[index % 3]),
                f"calibration instance {index} differs from declared seed")
    require(summary["manifest_sha256"] == digest(manifest), "manifest hash mismatch")
    require(summary["instances_sha256"] == digest(instances), "instance bundle hash mismatch")
    required_sources = {"benchmark.py", "independent_checker.py", "run_local_pilot.py"}
    require(set(manifest["source_sha256"]) == required_sources,
            "required source hash set mismatch")
    for name, expected in manifest["source_sha256"].items():
        require(name in {"benchmark.py", "independent_checker.py", "run_local_pilot.py"},
                "unexpected source file")
        source = Path(__file__).resolve().parent / name
        require(hashlib.sha256(source.read_bytes()).hexdigest() == expected,
                f"source hash mismatch: {name}")

    previous = "0" * 64
    policies: dict[str, dict[str, Any]] = {}
    observed: dict[tuple[str, str, int], int] = {}
    events = []
    completed: set[str] = set()
    selected: list[str] = []
    records: list[dict[str, Any]] = []
    declared_policies = expected_policies(seed, random_count)
    with (output_dir / "events.jsonl").open("rb") as journal:
        for index, raw_line in enumerate(journal):
            require(raw_line.endswith(b"\n"), "truncated event line")
            entry = strict_json(raw_line)
            require(set(entry) == {"sequence", "previous_sha256", "record", "sha256"},
                    "invalid event envelope")
            require(entry["sequence"] == index and entry["previous_sha256"] == previous,
                    "journal sequence or parent mismatch")
            payload = {key: value for key, value in entry.items() if key != "sha256"}
            current = hashlib.sha256(canonical_bytes(payload)).hexdigest()
            require(entry["sha256"] == current, "journal hash mismatch")
            previous = current
            record = entry["record"]
            records.append(record)
            events.append(record["event"])
            if record["event"] == "run_started":
                require(record["manifest_sha256"] == digest(manifest), "run manifest mismatch")
            elif record["event"] == "instances_frozen":
                require(record["bundle_sha256"] == digest(instances), "frozen instances mismatch")
                require(record["development_count"] == len(instances["development"])
                        and record["calibration_count"] == len(instances["calibration"]),
                        "frozen instance counts mismatch")
            if record["event"] == "candidate_started":
                name = record["candidate"]
                require(name in declared_policies and name not in policies,
                        "unknown or duplicate candidate")
                require(digest(record["policy"]) == record["policy_sha256"],
                        "policy hash mismatch")
                require(record["policy"] == declared_policies[name],
                        "candidate policy differs from declared deterministic roster")
                policies[name] = record["policy"]
            elif record["event"] == "policy_evaluated":
                phase, name, instance_index = (record[key] for key in
                    ("phase", "candidate", "instance_index"))
                require(phase in instances and name in policies, "unknown phase or candidate")
                require(type(instance_index) is int and 0 <= instance_index < len(instances[phase]),
                        "invalid instance index")
                key = (phase, name, instance_index)
                require(key not in observed, "duplicate policy evaluation")
                instance = instances[phase][instance_index]
                require(record["instance_sha256"] == digest(instance), "instance hash mismatch")
                entries = schedule(instance, policies[name])
                require(record["schedule_sha256"] == digest(entries), "schedule hash mismatch")
                score = check_schedule(instance, entries)
                require(record["makespan"] == score and record["status"] == "feasible",
                        "score mismatch")
                observed[key] = score
            elif record["event"] == "candidate_completed":
                name = record["candidate"]
                require(name in policies and name not in completed,
                        "unknown or twice-completed candidate")
                require(all(("development", name, i) in observed
                            for i in range(len(instances["development"]))),
                        "incomplete candidate development evaluations")
                scores = [observed[("development", name, i)]
                          for i in range(len(instances["development"]))]
                require(record["development_count"] == len(scores),
                        "incomplete candidate evaluation")
                require(record["development_mean_makespan"] == sum(scores) / len(scores),
                        "development mean mismatch")
                completed.add(name)
            elif record["event"] == "candidate_selected":
                selected.append(record["candidate"])
                require(record["basis"] == "development_only", "invalid selection basis")
            elif record["event"] == "run_completed":
                require(record["summary_sha256"] == digest(summary), "summary hash mismatch")

    require(events[:2] == ["run_started", "instances_frozen"] and events[-1] == "run_completed",
            "run start, freeze or completion missing")
    require(events.count("run_started") == events.count("instances_frozen") ==
            events.count("run_completed") == 1, "duplicate run boundary")
    require(events.count("candidate_started") == events.count("candidate_completed") ==
            summary["candidate_count"], "candidate lifecycle incomplete")
    expected_names = set(declared_policies)
    require(set(policies) == completed == expected_names, "candidate roster mismatch")
    random_names = [name for name in policies if name.startswith("random_")]
    best_random = (min(random_names, key=lambda name: (
        statistics.mean(observed[("development", name, index)]
                        for index in range(len(instances["development"]))), digest(policies[name])))
                   if random_names else None)
    require(selected == ([best_random] if best_random else []) and
            summary["selected_random_candidate"] == best_random, "selection mismatch")
    verify_event_order(records, list(declared_policies), best_random,
                       len(instances["development"]), len(instances["calibration"]))
    expected_calibration = {"earliest_finish", "shortest_duration", "remaining_work_first",
                            "least_loaded_machine"} | ({best_random} if best_random else set())
    require(set(summary["calibration"]) == expected_calibration,
            "calibration candidate set mismatch")
    require(summary["candidate_count"] == len(declared_policies),
            "summary candidate count mismatch")
    require(summary["development_evaluations"] ==
            len(declared_policies) * len(instances["development"]),
            "summary development budget mismatch")
    require(summary["calibration_evaluations"] ==
            len(expected_calibration) * len(instances["calibration"]),
            "summary calibration count mismatch")
    require(sum(key[0] == "development" for key in observed) ==
            summary["development_evaluations"], "development evaluation count mismatch")
    require(sum(key[0] == "calibration" for key in observed) ==
            summary["calibration_evaluations"], "calibration evaluation count mismatch")
    for name, result in summary["calibration"].items():
        scores = [observed[("calibration", name, i)]
                  for i in range(len(instances["calibration"]))]
        require(result["feasible_count"] == len(scores), "calibration feasibility mismatch")
        require(result["mean_makespan"] == sum(scores) / len(scores),
                "calibration mean mismatch")
        require(set(result["by_stratum"]) == set(STRATA),
                "calibration stratum set mismatch")
        for stratum, observed_mean in result["by_stratum"].items():
            values = [score for index, score in enumerate(scores)
                      if instances["calibration"][index]["stratum"] == stratum]
            require(values and observed_mean == sum(values) / len(values),
                    "calibration stratum mean mismatch")
    require(summary["model_requests"] == 0 and summary["provider_requests"] == 0,
            "external request count mismatch")
    if expected_head is not None:
        require(previous == expected_head, "published event head mismatch")
    return {"status": "verified", "stage": "offline_calibration", "formal_test": False,
            "event_count": len(events), "evaluation_count": len(observed),
            "last_event_sha256": previous, "summary_sha256": digest(summary),
            "model_requests": 0, "provider_requests": 0}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--expected-head", help="externally published final event SHA-256")
    arguments = parser.parse_args()
    print(json.dumps(verify(arguments.output_dir, arguments.expected_head),
                     sort_keys=True, ensure_ascii=True))


if __name__ == "__main__":
    main()
