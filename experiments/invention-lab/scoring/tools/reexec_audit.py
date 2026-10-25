#!/usr/bin/env python3
"""Re-execute and rescore every retained trial slot with the tool builds its
records name. Reads the evidence tree; writes only under a NEW WORK_DIR outside it.
Nonzero commands and malformed plans fail the audit; existing scratch results
are never accepted as fresh output.

    reexec_audit.py EVIDENCE_ROOT WORK_DIR LAB_SCORE [ENV=LAB_EXEC ...] [hidden:PREFIX=LAB_SCORE ...]

ENV is the first 8 hex digits of a plan's `development_environment` sha256;
LAB_EXEC is an executor build that reports that environment. A slot is
re-run with `lab-exec run` from the plan into a fresh state directory and its
receipt must equal the retained one except for measured wall time.

Scoring is rerun on the retained receipt (the evaluation names that receipt,
and a re-run differs in wall time). Where the retained slot kept no output
artifact, the re-executed output is used with the retained receipt; that is
only done after the re-run's receipt matched, so the output digest is the
retained one. Trials with a `descriptor.json` were scored on a hidden draw:
`hidden:PREFIX=LAB_SCORE` rescores those under PREFIX with `score-hidden`, the
salt revealed at `salt-revealed.txt` two levels above the run directory.

Invocation runs kept in `invoke.py`'s layout (one slot, `state/` and
`score/` beside `plan.json`) are audited the same way.

Every method run happens inside the executor's own sandbox; this script
needs whatever that sandbox needs (root, namespaces, the pinned python3).
Prints one JSON line per slot and writes WORK_DIR/audit.json.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys


def sha(data):
    return hashlib.sha256(data).hexdigest()


def comparable(path):
    value = json.load(open(path))
    value.get("usage", {}).pop("wall_time_ms", None)
    return value


def package_index(root):
    index = {}
    for d, _, files in os.walk(root):
        for f in files:
            if f.endswith(".json"):
                p = os.path.join(d, f)
                with open(p, "rb") as fh:
                    index.setdefault(sha(fh.read()), p)
    return index


def run(args):
    r = subprocess.run(args, capture_output=True, text=True, timeout=1800)
    message = (r.stdout + r.stderr).strip()[:160]
    if r.returncode != 0:
        raise RuntimeError(f"command exited {r.returncode}: {message}")
    return message


def layout(trial, sid, slots):
    """The retained state and score directories of one slot."""
    if len(slots) == 1 and os.path.exists(os.path.join(trial, "state", "receipt.json")):
        return os.path.join(trial, "state"), os.path.join(trial, "score")
    return os.path.join(trial, sid), os.path.join(trial, sid + "-score")


def rescore(lab_score, trial, sid, state, out, hidden_root, hidden_score, retained):
    plan = os.path.join(trial, "plan.json")
    if hidden_root:
        salt = os.path.join(hidden_root, "salt-revealed.txt")
        slots = [f"{s}={os.path.join(trial, s)}" for s in ("candidate", "control")
                 if os.path.exists(os.path.join(trial, s, "receipt.json"))]
        msg = run([hidden_score, "score-hidden", plan, sid, os.path.join(trial, "descriptor.json"),
                   salt, out, *slots])
    else:
        msg = run([lab_score, "score", plan, sid, state, out])
    fresh = os.path.join(out, "evaluation.json")
    if not os.path.exists(fresh):
        return "failed: " + msg
    return "same" if comparable(fresh) == comparable(retained) else "DIFFERENT"


def main(argv):
    root, work, lab_score, *maps = argv
    root, work = os.path.realpath(root), os.path.realpath(work)
    # No old receipt or scoring file may stand in for a new command result.
    # Refuse a directory inside the evidence root too: it is read-only input.
    if os.path.commonpath([root, work]) == root:
        print("WORK_DIR must be outside EVIDENCE_ROOT", file=sys.stderr)
        return 1
    try:
        os.makedirs(work, mode=0o700, exist_ok=False)
    except OSError:
        print("WORK_DIR must be a new directory", file=sys.stderr)
        return 1
    executors = dict(m.split("=", 1) for m in maps if not m.startswith("hidden:"))
    hidden = dict(m[len("hidden:"):].split("=", 1) for m in maps if m.startswith("hidden:"))
    packages = package_index(root)
    trials = sorted(d for d, _, f in os.walk(root) if "plan.json" in f)
    rows = []
    for n, trial in enumerate(trials):
        try:
            with open(os.path.join(trial, "plan.json")) as fh:
                plan = json.load(fh)
            if not isinstance(plan, dict) or not isinstance(plan.get("slots"), list) or not plan["slots"]:
                raise ValueError("invalid plan slots")
        except (OSError, ValueError) as error:
            rows.append({"trial": os.path.relpath(trial, root),
                         "exec": "invalid retained plan: " + type(error).__name__})
            continue
        env = plan.get("development_environment", {}).get("sha256", "")[:8]
        rel = os.path.relpath(trial, root)
        is_hidden = os.path.exists(os.path.join(trial, "descriptor.json"))
        hidden_score = next((v for k, v in hidden.items() if rel.startswith(k)), None)
        for slot in plan.get("slots", []):
            sid = slot["slot_id"]
            slot_state, score_dir = layout(trial, sid, plan["slots"])
            retained = os.path.join(slot_state, "receipt.json")
            if not os.path.exists(retained):
                continue
            row = {"trial": rel, "slot": sid, "env": env}
            rows.append(row)
            exe, pkg = executors.get(env), packages.get(slot["method"]["sha256"])
            if not exe or not pkg:
                row["exec"] = "no executor build" if not exe else "no package"
                print(json.dumps(row), flush=True)
                continue
            state = os.path.join(work, f"{n:03d}-{sid}")
            try:
                msg = run([exe, "run", os.path.join(trial, "plan.json"), sid, pkg, state, "namespaces"])
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                row["exec"] = "failed: " + str(error)[:160]
                print(json.dumps(row), flush=True)
                continue
            fresh = os.path.join(state, "receipt.json")
            if not os.path.exists(fresh):
                row["exec"] = "failed: " + msg
            elif comparable(fresh) != comparable(retained):
                row["exec"] = "DIFFERENT"
            else:
                row["exec"] = "same"
                evaluation = os.path.join(score_dir, "evaluation.json")
                if not os.path.exists(evaluation):
                    row["score"] = "none retained"
                elif is_hidden and not hidden_score:
                    row["score"] = "hidden, no scorer given"
                else:
                    scored_state = slot_state
                    outputs = json.load(open(retained)).get("outputs") or []
                    missing = [o["sha256"] for o in outputs
                               if not os.path.exists(os.path.join(scored_state, "artifacts", o["sha256"]))]
                    if missing:
                        scored_state = state + "-retained-receipt"
                        shutil.rmtree(scored_state, ignore_errors=True)
                        shutil.copytree(os.path.join(state, "artifacts"), os.path.join(scored_state, "artifacts"))
                        shutil.copy(retained, os.path.join(scored_state, "receipt.json"))
                        row["output"] = "re-executed (not retained)"
                    hidden_root = os.path.dirname(os.path.dirname(trial)) if is_hidden else None
                    try:
                        row["score"] = rescore(lab_score, trial, sid, scored_state, state + "-score",
                                               hidden_root, hidden_score, evaluation)
                    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                        row["score"] = "failed: " + str(error)[:160]
            print(json.dumps(row), flush=True)
    with open(os.path.join(work, "audit.json"), "w") as fh:
        json.dump(rows, fh, indent=1)
    bad = [r for r in rows if r.get("exec") != "same" or r.get("score") not in ("same", "none retained")]
    return 1 if bad or not rows else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
