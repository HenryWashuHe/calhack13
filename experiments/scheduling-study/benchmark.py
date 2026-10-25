"""Versioned, deterministic flexible job-shop study fixtures and policy runner.

All outputs are prospective synthetic research fixtures, not formal test evidence.
"""

from __future__ import annotations

import hashlib
import json
from typing import Any


GENERATOR_VERSION = "scheduling-study/1"
FEATURES = ("finish", "duration", "remaining", "machine_load", "flexibility", "wait")
STRATA = ("balanced", "bottleneck", "restricted")
JOB_COUNT = 60
OPERATIONS_PER_JOB = 6
MACHINE_COUNT = 12


class DeterministicDraw:
    def __init__(self, seed: int, stratum: str) -> None:
        if type(seed) is not int or seed < 0 or seed > 2**64 - 1:
            raise ValueError("seed must be an unsigned 64-bit integer")
        if stratum not in STRATA:
            raise ValueError("unknown stratum")
        self.prefix = f"{GENERATOR_VERSION}|{stratum}|{seed}|".encode("ascii")
        self.counter = 0

    def below(self, upper: int) -> int:
        if upper <= 0:
            raise ValueError("upper bound must be positive")
        digest = hashlib.sha256(self.prefix + str(self.counter).encode("ascii")).digest()
        self.counter += 1
        return int.from_bytes(digest, "big") % upper

    def pick(self, items: list[int], count: int) -> list[int]:
        if count > len(items) or count < 0:
            raise ValueError("invalid selection count")
        pool = items.copy()
        selected = []
        for _ in range(count):
            selected.append(pool.pop(self.below(len(pool))))
        return sorted(selected)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def digest(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def generate_instance(seed: int, stratum: str) -> dict[str, Any]:
    draw = DeterministicDraw(seed, stratum)
    jobs: list[list[list[dict[str, int]]]] = []
    for _job in range(JOB_COUNT):
        operations = []
        for _operation in range(OPERATIONS_PER_JOB):
            count = 2 + draw.below(3)
            pool = list(range(MACHINE_COUNT if stratum != "restricted" else 6))
            machines = draw.pick(pool, count)
            alternatives = []
            for machine in machines:
                duration = 1 + draw.below(30)
                if stratum == "bottleneck":
                    duration = 1 + draw.below(8) if machine < 3 else 20 + draw.below(11)
                alternatives.append({"machine": machine, "duration": duration})
            operations.append(alternatives)
        jobs.append(operations)
    return {
        "schema": GENERATOR_VERSION,
        "seed": seed,
        "stratum": stratum,
        "machine_count": MACHINE_COUNT,
        "jobs": jobs,
    }


def validate_policy(policy: dict[str, Any]) -> tuple[int, ...]:
    if not isinstance(policy, dict) or set(policy) != {"schema", "weights"}:
        raise ValueError("policy must contain only schema and weights")
    if policy["schema"] != "linear-dispatch/1":
        raise ValueError("unsupported policy schema")
    weights = policy["weights"]
    if not isinstance(weights, dict) or set(weights) != set(FEATURES):
        raise ValueError("policy weights must name the six fixed features")
    values = tuple(weights[name] for name in FEATURES)
    if any(type(weight) is not int or weight < -8 or weight > 8 for weight in values):
        raise ValueError("policy weights must be integers in [-8, 8]")
    if all(weight == 0 for weight in values):
        raise ValueError("policy must have a nonzero weight")
    return values


def make_policy(weights: tuple[int, ...]) -> dict[str, Any]:
    if len(weights) != len(FEATURES):
        raise ValueError("incorrect feature count")
    policy = {"schema": "linear-dispatch/1", "weights": dict(zip(FEATURES, weights))}
    validate_policy(policy)
    return policy


def schedule(instance: dict[str, Any], policy: dict[str, Any]) -> list[dict[str, int]]:
    """Construct a complete, deterministic earliest-start serial schedule."""
    weights = validate_policy(policy)
    if instance.get("schema") != GENERATOR_VERSION or instance.get("machine_count") != MACHINE_COUNT:
        raise ValueError("unsupported instance")
    jobs = instance["jobs"]
    if len(jobs) != JOB_COUNT or any(len(job) != OPERATIONS_PER_JOB for job in jobs):
        raise ValueError("incorrect job or operation count")
    remaining = [
        [sum(min(choice["duration"] for choice in op) for op in job[index:])
         for index in range(OPERATIONS_PER_JOB)]
        for job in jobs
    ]
    next_operation = [0] * JOB_COUNT
    job_ready = [0] * JOB_COUNT
    machine_ready = [0] * MACHINE_COUNT
    machine_load = [0] * MACHINE_COUNT
    result: list[dict[str, int]] = []

    for _ in range(JOB_COUNT * OPERATIONS_PER_JOB):
        best_key: tuple[int, int, int, int] | None = None
        best_step: tuple[int, int, int, int, int] | None = None
        for job_id, job in enumerate(jobs):
            operation_id = next_operation[job_id]
            if operation_id == OPERATIONS_PER_JOB:
                continue
            alternatives = job[operation_id]
            for choice in alternatives:
                machine = choice["machine"]
                duration = choice["duration"]
                start = max(job_ready[job_id], machine_ready[machine])
                values = (start + duration, duration, remaining[job_id][operation_id],
                          machine_load[machine], len(alternatives), start - job_ready[job_id])
                priority = sum(weight * value for weight, value in zip(weights, values))
                key = (priority, job_id, operation_id, machine)
                if best_key is None or key < best_key:
                    best_key = key
                    best_step = (job_id, operation_id, machine, start, duration)
        assert best_step is not None
        job_id, operation_id, machine, start, duration = best_step
        result.append({"job": job_id, "operation": operation_id, "machine": machine,
                       "start": start, "duration": duration})
        next_operation[job_id] += 1
        job_ready[job_id] = start + duration
        machine_ready[machine] = start + duration
        machine_load[machine] += duration
    return result
