"""Verify an offline pilot's complete journal, artifacts and scored schedules."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import statistics
from typing import Any

from benchmark import canonical_bytes, digest, schedule
from independent_checker import check_schedule


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="ascii"),
                      parse_constant=lambda value: (_ for _ in ()).throw(
                          ValueError(f"non-finite JSON constant: {value}")))


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def verify(output_dir: Path) -> dict[str, Any]:
    manifest = read_json(output_dir / "manifest.json")
    instances = read_json(output_dir / "instances.json")
    summary = read_json(output_dir / "summary.json")
    require(manifest["schema"] == "scheduling-study-offline-pilot/1", "wrong manifest schema")
    require(manifest["formal_test"] is False and summary["formal_test"] is False,
            "offline pilot must not claim formal evidence")
    require(manifest["model_requests"] == 0 and manifest["provider_requests"] == 0,
            "offline pilot must have zero external requests")
    require(len(instances["development"]) == manifest["development_instances"],
            "development instance count mismatch")
    require(len(instances["calibration"]) == manifest["calibration_instances"],
            "calibration instance count mismatch")
    require(summary["manifest_sha256"] == digest(manifest), "manifest hash mismatch")
    require(summary["instances_sha256"] == digest(instances), "instance bundle hash mismatch")
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
    with (output_dir / "events.jsonl").open("rb") as journal:
        for index, raw_line in enumerate(journal):
            require(raw_line.endswith(b"\n"), "truncated event line")
            entry = json.loads(raw_line, parse_constant=lambda _: (_ for _ in ()).throw(
                ValueError("non-finite event number")))
            require(set(entry) == {"sequence", "previous_sha256", "record", "sha256"},
                    "invalid event envelope")
            require(entry["sequence"] == index and entry["previous_sha256"] == previous,
                    "journal sequence or parent mismatch")
            payload = {key: value for key, value in entry.items() if key != "sha256"}
            current = hashlib.sha256(canonical_bytes(payload)).hexdigest()
            require(entry["sha256"] == current, "journal hash mismatch")
            previous = current
            record = entry["record"]
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
                require(name not in policies, "duplicate candidate")
                require(digest(record["policy"]) == record["policy_sha256"],
                        "policy hash mismatch")
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
    expected_names = {"earliest_finish", "shortest_duration", "remaining_work_first",
                      "least_loaded_machine"} | {
                          f"random_{index:03d}" for index in range(manifest["random_candidates"])}
    require(set(policies) == completed == expected_names, "candidate roster mismatch")
    random_names = [name for name in policies if name.startswith("random_")]
    best_random = (min(random_names, key=lambda name: (
        statistics.mean(observed[("development", name, index)]
                        for index in range(len(instances["development"]))), digest(policies[name])))
                   if random_names else None)
    require(selected == ([best_random] if best_random else []) and
            summary["selected_random_candidate"] == best_random, "selection mismatch")
    expected_calibration = {"earliest_finish", "shortest_duration", "remaining_work_first",
                            "least_loaded_machine"} | ({best_random} if best_random else set())
    require(set(summary["calibration"]) == expected_calibration,
            "calibration candidate set mismatch")
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
        for stratum, observed_mean in result["by_stratum"].items():
            values = [score for index, score in enumerate(scores)
                      if instances["calibration"][index]["stratum"] == stratum]
            require(values and observed_mean == sum(values) / len(values),
                    "calibration stratum mean mismatch")
    require(summary["model_requests"] == 0 and summary["provider_requests"] == 0,
            "external request count mismatch")
    return {"status": "verified", "stage": "offline_calibration", "formal_test": False,
            "event_count": len(events), "evaluation_count": len(observed),
            "last_event_sha256": previous, "summary_sha256": digest(summary),
            "model_requests": 0, "provider_requests": 0}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    arguments = parser.parse_args()
    print(json.dumps(verify(arguments.output_dir), sort_keys=True, ensure_ascii=True))


if __name__ == "__main__":
    main()
