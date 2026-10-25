"""Run a source-only calibration; never contacts a model or external provider."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import statistics
import time
from typing import Any

from benchmark import (DeterministicDraw, STRATA, canonical_bytes, digest,
                       generate_instance, make_policy, schedule)
from independent_checker import check_schedule


BASELINES = {
    "earliest_finish": (1, 0, 0, 0, 0, 0),
    "shortest_duration": (0, 1, 0, 0, 0, 0),
    "remaining_work_first": (0, 0, -1, 0, 0, 0),
    "least_loaded_machine": (0, 0, 0, 1, 0, 0),
}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def write_new_json(path: Path, value: Any) -> None:
    with path.open("xb") as output:
        output.write(canonical_bytes(value) + b"\n")
        output.flush()
        os.fsync(output.fileno())


class EventJournal:
    def __init__(self, path: Path) -> None:
        self.output = path.open("xb")
        self.sequence = 0
        self.previous = "0" * 64

    def append(self, event: str, **fields: Any) -> None:
        record = {"event": event, "timestamp_utc": utc_now(), **fields}
        body = {"sequence": self.sequence, "previous_sha256": self.previous, "record": record}
        current = hashlib.sha256(canonical_bytes(body)).hexdigest()
        self.output.write(canonical_bytes({**body, "sha256": current}) + b"\n")
        self.output.flush()
        os.fsync(self.output.fileno())
        self.sequence += 1
        self.previous = current

    def close(self) -> None:
        self.output.close()


def evaluate(policy: dict[str, Any], instances: list[dict[str, Any]],
             journal: EventJournal, phase: str, candidate: str) -> list[int]:
    scores = []
    for index, instance in enumerate(instances):
        start_ns = time.monotonic_ns()
        artifact = schedule(instance, policy)
        score = check_schedule(instance, artifact)
        elapsed_ns = time.monotonic_ns() - start_ns
        journal.append("policy_evaluated", phase=phase, candidate=candidate,
                       instance_index=index, instance_sha256=digest(instance),
                       schedule_sha256=digest(artifact), makespan=score,
                       elapsed_ns=elapsed_ns, status="feasible")
        scores.append(score)
    return scores


def run_pilot(output_dir: Path, seed: int, random_candidates: int) -> dict[str, Any]:
    if type(random_candidates) is not int or not 0 <= random_candidates <= 200:
        raise ValueError("random_candidates must be between 0 and 200")
    if type(seed) is not int or not 0 <= seed <= 2**64 - 1_000_001:
        raise ValueError("seed out of range")
    output_dir.mkdir(parents=True, exist_ok=False)
    source_dir = Path(__file__).resolve().parent
    source_hashes = {name: hashlib.sha256((source_dir / name).read_bytes()).hexdigest()
                     for name in ("benchmark.py", "independent_checker.py", "run_local_pilot.py")}
    manifest = {
        "schema": "scheduling-study-offline-pilot/1",
        "stage": "offline_calibration",
        "started_utc": utc_now(),
        "seed": seed,
        "random_candidates": random_candidates,
        "development_instances": 20,
        "calibration_instances": 32,
        "strata": list(STRATA),
        "source_sha256": source_hashes,
        "selection_rule": "lowest mean development makespan; canonical policy hash breaks ties",
        "model_requests": 0,
        "provider_requests": 0,
        "formal_test": False,
    }
    write_new_json(output_dir / "manifest.json", manifest)
    journal = EventJournal(output_dir / "events.jsonl")
    start_ns = time.monotonic_ns()
    try:
        journal.append("run_started", manifest_sha256=digest(manifest))
        development = [generate_instance(seed + index, STRATA[index % 3]) for index in range(20)]
        calibration = [generate_instance(seed + 10_000 + index, STRATA[index % 3])
                       for index in range(32)]
        instance_bundle = {"development": development, "calibration": calibration}
        write_new_json(output_dir / "instances.json", instance_bundle)
        journal.append("instances_frozen", bundle_sha256=digest(instance_bundle),
                       development_count=20, calibration_count=32)

        candidates = {name: make_policy(weights) for name, weights in BASELINES.items()}
        draw = DeterministicDraw(seed + 1_000_000, "balanced")
        for index in range(random_candidates):
            weights = tuple(draw.below(7) - 3 for _ in range(6))
            if not any(weights):
                weights = (1, 0, 0, 0, 0, 0)
            candidates[f"random_{index:03d}"] = make_policy(weights)

        development_scores = {}
        for name, policy in candidates.items():
            journal.append("candidate_started", candidate=name, policy_sha256=digest(policy),
                           policy=policy)
            scores = evaluate(policy, development, journal, "development", name)
            development_scores[name] = statistics.mean(scores)
            journal.append("candidate_completed", candidate=name,
                           development_mean_makespan=development_scores[name],
                           development_count=len(scores))

        random_names = [name for name in candidates if name.startswith("random_")]
        selected_random = (min(random_names,
                               key=lambda name: (development_scores[name], digest(candidates[name])))
                           if random_names else None)
        selected = list(BASELINES)
        if selected_random is not None:
            selected.append(selected_random)
            journal.append("candidate_selected", candidate=selected_random,
                           basis="development_only",
                           development_mean_makespan=development_scores[selected_random])

        calibration_scores = {}
        for name in selected:
            scores = evaluate(candidates[name], calibration, journal, "calibration", name)
            calibration_scores[name] = {
                "mean_makespan": statistics.mean(scores),
                "by_stratum": {
                    stratum: statistics.mean(
                        score for index, score in enumerate(scores) if STRATA[index % 3] == stratum)
                    for stratum in STRATA
                },
                "feasible_count": len(scores),
            }

        summary = {
            "schema": "scheduling-study-offline-pilot-result/1",
            "stage": "offline_calibration",
            "formal_test": False,
            "manifest_sha256": digest(manifest),
            "instances_sha256": digest(instance_bundle),
            "candidate_count": len(candidates),
            "development_evaluations": len(candidates) * len(development),
            "calibration_evaluations": len(selected) * len(calibration),
            "selected_random_candidate": selected_random,
            "calibration": calibration_scores,
            "model_requests": 0,
            "provider_requests": 0,
            "elapsed_ns": time.monotonic_ns() - start_ns,
        }
        write_new_json(output_dir / "summary.json", summary)
        journal.append("run_completed", summary_sha256=digest(summary),
                       candidate_count=len(candidates),
                       development_evaluations=summary["development_evaluations"],
                       calibration_evaluations=summary["calibration_evaluations"],
                       model_requests=0, provider_requests=0)
        return summary
    except Exception as error:
        journal.append("run_failed", error_type=type(error).__name__, error_message=str(error))
        raise
    finally:
        journal.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--seed", type=int, default=81000)
    parser.add_argument("--random-candidates", type=int, default=40)
    arguments = parser.parse_args()
    summary = run_pilot(arguments.output_dir, arguments.seed, arguments.random_candidates)
    print(json.dumps(summary, sort_keys=True, ensure_ascii=True))


if __name__ == "__main__":
    main()
