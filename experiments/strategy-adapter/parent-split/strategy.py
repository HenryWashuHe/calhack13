"""Screen-then-confirm search, split out of generated method 8215c367.

Domain-free: this file knows items, candidates, an `evaluate` callback that
returns (passed, progress) or None, and nothing about what they are. The four
parts a mutator may replace independently are order, propose, allocate, stop;
`search` only composes them. Behavior is the parent's, byte for byte, on the
retained rough-transport transcript (see ../harness).
"""
import itertools

# Evaluation cap the parent used (`nexp >= 32`); enforced by the caller's
# evaluator so the cap is checked at exactly the parent's point.
MAX_EVALUATIONS = 32


def order(items, hardness):
    """Hardest first. Stable: equal hardness keeps the given order."""
    return sorted(items, key=hardness, reverse=True)


def propose(axes, realize):
    """Every grid point in lexicographic axis order that `realize` accepts."""
    out = []
    for point in itertools.product(*axes):
        c = realize(point)
        if c is not None:
            out.append(c)
    return out


def allocate(results, n):
    """Index of the next item for one candidate, or None when it is done.

    Screen on item 0; confirm on the rest only after the screen passed."""
    if not results:
        return 0
    if not results[0][0]:
        return None
    return len(results) if len(results) < n else None


def stop(wins, n):
    """Stop the whole search once a candidate passed every item."""
    return wins == n


def search(items, candidates, evaluate, halted):
    best = None
    best_key = (-1, -1.0)
    n = len(items)
    for c in candidates:
        if halted():
            break
        results = []
        while True:
            k = allocate(results, n)
            if k is None:
                break
            if results and halted():
                break
            r = evaluate(items[k], c)
            if r is None:
                break
            results.append(r)
        if not results:
            break
        wins = sum(1 for passed, _ in results if passed)
        key = (wins, results[0][1])
        if key > best_key:
            best_key = key
            best = c
        if stop(wins, n):
            break
    return best
