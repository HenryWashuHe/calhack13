#!/usr/bin/env python3
"""The one mixed-terrain/1 confirmation run (proposal #195), on seeds
51000..51071, twelve panels of six, with the frozen comparison.

    confirm.py <lab-eval-mixed> > ../confirmation/result.json

Same arms and harness as `run.py`: `random_search` and `portfolio_search`
through the reviewed `calibrate.py` offline emulation (not the executor, not
trial evidence), the two fixed designs through `lab-eval-mixed accept` for
reference. The predeclared reading: *discriminating* if `portfolio_search`
beats `random_search` by margin one on at least 7 of 12 panels and loses by
margin one on at most 2, with all 12 panels available for both arms;
*incomplete* if any panel is unavailable for either arm (published as is,
never rerun); otherwise *not discriminating*. It refuses to run if the
evaluator identity differs from the one passed as the frozen digest.
"""
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402

calibrate = run.calibrate
FIRST, PANELS, GRANT = 51000, 12, 72000
WINS_NEEDED, LOSSES_ALLOWED = 7, 2


def reading(a, b):
    if None in a or None in b:
        missing = [i for i, (x, y) in enumerate(zip(a, b)) if x is None or y is None]
        return {"reading": "incomplete", "unavailable_panels": missing}
    wins = sum(x >= y + 1 for x, y in zip(a, b))
    losses = sum(y >= x + 1 for x, y in zip(a, b))
    ok = wins >= WINS_NEEDED and losses <= LOSSES_ALLOWED
    return {"reading": "discriminating" if ok else "not discriminating",
            "wins": wins, "losses": losses, "of": len(a)}


def main(argv):
    lab_eval, frozen = argv[1], argv[2]
    ident = calibrate.lab(lab_eval, "identity")
    if ident.get("evaluator_sha256") != frozen:
        raise SystemExit(f"evaluator identity {ident.get('evaluator_sha256')} is not the frozen {frozen}")
    panels = [list(range(FIRST + 6 * p, FIRST + 6 * p + 6)) for p in range(PANELS)]
    out = {"family": run.FAMILY, "evaluator": ident, "seeds": [FIRST, FIRST + 6 * PANELS - 1],
           "grant": GRANT, "arms": {}}
    with tempfile.TemporaryDirectory() as tmp:
        for name, path in run.METHODS.items():
            rows = [calibrate.panel_run(lab_eval, path, run.FAMILY, seeds, GRANT, tmp)
                    for seeds in panels]
            out["arms"][name] = {"panels": rows, "per_panel": [r["successes"] for r in rows],
                                 "total": calibrate.total(rows)}
        for name, path in run.FIXED.items():
            rows = []
            for seeds in panels:
                row = {"seeds": [seeds[0], seeds[-1]]}
                try:
                    scored = calibrate.lab(lab_eval, "accept", path, run.FAMILY, *map(str, seeds))
                    row["successes"] = calibrate.acceptance_count(scored, seeds)
                except calibrate.Unavailable as e:
                    row["successes"] = None
                    row["unavailable"] = str(e)
                rows.append(row)
            per = [r["successes"] for r in rows]
            out["arms"][name] = {"panels": rows, "per_panel": per,
                                 "total": None if None in per else sum(per)}
    out["result"] = reading(out["arms"]["portfolio_search"]["per_panel"],
                            out["arms"]["random_search"]["per_panel"])
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
