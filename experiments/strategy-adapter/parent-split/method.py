"""Entry point: binds the domain-free strategy to the genesis-lab adapter.

Hand split of generated method 8215c367 (docs/invention-lab/evidence/
live-method-20261024-generated/method-package.json). Not model-generated,
not an improvement: the acceptance test is that it behaves identically.
"""
import sys

sys.dont_write_bytecode = True

import adapter  # noqa: E402
import strategy  # noqa: E402


def main():
    s = adapter.Session(strategy.MAX_EVALUATIONS)
    items = s.items()
    fallback = adapter.default()
    if not items:
        s.submit(fallback)
        return
    cap = adapter.budget(items)
    items = strategy.order(items, adapter.hardness)
    candidates = strategy.propose(adapter.AXES, adapter.realizer(cap)) or [fallback]
    best = strategy.search(items, candidates, s.evaluate, s.halted)
    s.submit(best if best is not None else fallback)


try:
    main()
except Exception:
    pass
sys.exit(0)
