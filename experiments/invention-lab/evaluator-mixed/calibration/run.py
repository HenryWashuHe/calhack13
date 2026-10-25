#!/usr/bin/env python3
"""One calibration run of mixed-terrain/1 (proposal #195): the four
hand-written controls on calibration seeds 50000..50035, six panels of six.

    run.py <lab-eval-mixed> <label> > runs/<n>-<label>.json

`random_search` and `portfolio_search` go through the reviewed `calibrate.py`
offline emulation of the executor protocol (not the executor, not trial
evidence). The parent's fixed cart and the five-wheel reference are scored
with `lab-eval-mixed accept` only. A failed, slow or malformed measurement is
`null` with its reason, never zero. No generated source runs here.
"""
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))
ANALYSIS = os.path.join(ROOT, "docs", "invention-lab", "analysis", "family-robustness")
sys.path.insert(0, ANALYSIS)
import calibrate  # noqa: E402
import headroom  # noqa: E402

FAMILY = "mixed-terrain"
FIRST, PANELS, GRANT = 50000, 6, 72000
METHODS = headroom.METHODS
FIXED = headroom.FIXED


def main(argv):
    lab_eval, label = argv[1], argv[2]
    ident = calibrate.lab(lab_eval, "identity")
    panels = [list(range(FIRST + 6 * p, FIRST + 6 * p + 6)) for p in range(PANELS)]
    out = {"label": label, "family": FAMILY, "evaluator": ident,
           "seeds": [FIRST, FIRST + 6 * PANELS - 1], "grant": GRANT, "arms": {}}
    with tempfile.TemporaryDirectory() as tmp:
        for name, path in METHODS.items():
            runs = [calibrate.panel_run(lab_eval, path, FAMILY, seeds, GRANT, tmp)
                    for seeds in panels]
            out["arms"][name] = {"panels": runs, "per_panel": [r["successes"] for r in runs],
                                 "total": calibrate.total(runs)}
        for name, path in FIXED.items():
            rows = []
            for seeds in panels:
                row = {"seeds": [seeds[0], seeds[-1]]}
                try:
                    scored = calibrate.lab(lab_eval, "accept", path, FAMILY, *map(str, seeds))
                    row["successes"] = calibrate.acceptance_count(scored, seeds)
                except calibrate.Unavailable as e:
                    row["successes"] = None
                    row["unavailable"] = str(e)
                rows.append(row)
            per = [r["successes"] for r in rows]
            out["arms"][name] = {"panels": rows, "per_panel": per,
                                 "total": None if None in per else sum(per)}
    a, b = out["arms"]["portfolio_search"]["per_panel"], out["arms"]["random_search"]["per_panel"]
    if None in a or None in b:
        out["portfolio_vs_random"] = None
    else:
        out["portfolio_vs_random"] = {"wins": sum(x >= y + 1 for x, y in zip(a, b)),
                                      "losses": sum(y >= x + 1 for x, y in zip(a, b)),
                                      "of": len(a)}
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main(sys.argv)
