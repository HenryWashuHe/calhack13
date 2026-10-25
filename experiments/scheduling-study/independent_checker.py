"""Independent structural validator for scheduling-study/1 result artifacts."""

from __future__ import annotations

from typing import Any


def check_schedule(instance: dict[str, Any], entries: Any) -> int:
    """Return makespan; reject every missing, invalid or overlapping operation."""
    if not isinstance(instance, dict) or instance.get("schema") != "scheduling-study/1":
        raise ValueError("unsupported instance schema")
    machine_count = instance.get("machine_count")
    if type(machine_count) is not int or machine_count != 12:
        raise ValueError("invalid machine count")
    jobs = instance.get("jobs")
    if not isinstance(jobs, list) or len(jobs) != 60:
        raise ValueError("invalid job count")
    if any(not isinstance(job, list) or len(job) != 6 for job in jobs):
        raise ValueError("invalid operation count")
    if not isinstance(entries, list) or len(entries) != 360:
        raise ValueError("schedule must contain exactly 360 operations")

    seen: set[tuple[int, int]] = set()
    by_job: list[list[tuple[int, int] | None]] = [[None] * 6 for _ in range(60)]
    by_machine: list[list[tuple[int, int]]] = [[] for _ in range(12)]
    makespan = 0
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"job", "operation", "machine", "start", "duration"}:
            raise ValueError("invalid schedule entry fields")
        job, operation, machine, start, duration = (entry[key] for key in
            ("job", "operation", "machine", "start", "duration"))
        if any(type(value) is not int for value in (job, operation, machine, start, duration)):
            raise ValueError("schedule fields must be integers")
        if not (0 <= job < 60 and 0 <= operation < 6 and 0 <= machine < 12):
            raise ValueError("schedule identity out of range")
        if start < 0 or duration <= 0 or start + duration > 1_000_000:
            raise ValueError("invalid start, duration or time limit")
        identity = (job, operation)
        if identity in seen:
            raise ValueError("duplicate operation")
        seen.add(identity)
        alternatives = jobs[job][operation]
        if not isinstance(alternatives, list) or not (2 <= len(alternatives) <= 4):
            raise ValueError("invalid instance alternatives")
        allowed = []
        for choice in alternatives:
            if not isinstance(choice, dict) or set(choice) != {"machine", "duration"}:
                raise ValueError("invalid instance choice")
            selected_machine, selected_duration = choice["machine"], choice["duration"]
            if (type(selected_machine) is not int or type(selected_duration) is not int
                    or not 0 <= selected_machine < 12 or not 1 <= selected_duration <= 30):
                raise ValueError("invalid instance machine or duration")
            allowed.append((selected_machine, selected_duration))
        if len({selected_machine for selected_machine, _ in allowed}) != len(allowed):
            raise ValueError("duplicate machine alternative")
        if (machine, duration) not in allowed:
            raise ValueError("ineligible machine or incorrect duration")
        end = start + duration
        by_job[job][operation] = (start, end)
        by_machine[machine].append((start, end))
        makespan = max(makespan, end)

    if len(seen) != 360:
        raise ValueError("missing operation")
    for operations in by_job:
        for previous, following in zip(operations, operations[1:]):
            assert previous is not None and following is not None
            if previous[1] > following[0]:
                raise ValueError("job precedence violation")
    for intervals in by_machine:
        intervals.sort()
        for previous, following in zip(intervals, intervals[1:]):
            if previous[1] > following[0]:
                raise ValueError("machine overlap")
    return makespan
