# Proposal: strategy-adapter split spec (B)

**Status:** proposal — not a completion-gate claim.
**Scope:** `experiments/strategy-adapter/**` (Eric's research lane).
**Based on:** parent `8215c367`, branch `eric/strategy-adapter-split`.

---

## 1. Motivation

The generated parent `8215c367` bundles search strategy (how to order, propose,
allocate, stop) with domain adapter (line protocol, feature extraction, candidate
space, design construction) in one 175-line file. This tangling made it
impossible to mutate one independently. The split separates them so that a
mutator can target either part in isolation.

---

## 2. Interfaces as implemented

### 2.1 `strategy.py`

Domain-free. Knows items, candidates, an `evaluate` callback, and nothing about
what they are.

| Symbol | Signature | Contract |
|---|---|---|
| `order(items, hardness)` | `(list, callable) → list` | Sort hardest-first, stable on ties. |
| `propose(axes, realize)` | `(list[list], callable) → list` | Every grid point in lex order that `realize` accepts. |
| `allocate(results, n)` | `(list, int) → int \| None` | Screen on item 0; confirm remaining only if screen passed. |
| `stop(wins, n)` | `(int, int) → bool` | True once a candidate passed every item. |
| `search(items, candidates, evaluate, halted)` | `(list, list, callable, callable) → any` | Compose the four above; return the best candidate seen. |
| `MAX_EVALUATIONS` | `int = 32` | Evaluation cap; enforced by the caller's evaluator. |

A mutator may replace `order`, `propose`, `allocate`, `stop`, or `search`
independently, or all of them, as long as `search` satisfies the acceptance
rule (§4).

### 2.2 `adapter.py`

Everything domain-specific; nothing searches.

| Symbol | Purpose | Mutator may change? |
|---|---|---|
| `hardness(item)` | Returns the instance feature used by `order`. | Yes — but see Finding 1 (§5). |
| `budget(items)` | Reads the cost cap from instance rules. | No — protocol. |
| `AXES` | Wheel radius, motor speed, deck length, wheel material. | Yes — values and lengths. |
| `realizer(cap)` | Converts an `AXES` point to a design, or `None` if infeasible. | No without changing `build`. |
| `build(r, speed, L, wm)` | Assembles the design dict. Returns `(design, cost)`. | No without changing `AXES`. |
| `MAT` | Material density lookup. | No — protocol. |
| `send(req)` | Line-protocol I/O. | No — protocol. |
| `Session` | Protocol state (remaining steps, call count, halt flag). | No — protocol. |
| `Session.items()` | Fetch instances via `send`. | No — protocol. |
| `Session.evaluate(item, design)` | Run one experiment. | No — protocol. |
| `Session.submit(design)` | Submit final design. | No — protocol. |
| `default()` | Fallback design when search yields nothing. | Yes. |

### 2.3 `method.py`

Glue only. Imports `strategy` and `adapter`; runs `adapter.Session`, passes its
callbacks into `strategy.search`, submits the result. No logic; a mutator does
not replace it.

---

## 3. Identity

The **package hash** is derived from all three files together by the executor's
`package` command:

```
sha256 (package): a0cf67e2d003afaaf79f1b9fd36aefeee8e419945aae7ad4403e46471e6a017b
```

Two **derived hashes** are kept as a sidecar in
`experiments/strategy-adapter/parent-split/` for reference until Codex decides
on a contract change:

```
strategy.py sha256: 0669820a4f9cc1f36a7a1b6f91ab31defcd0e774684c28e311bab64fb1fa7133
adapter.py  sha256: 0fc4b50140b967885d414183ca84166286fb39afe7b0dd49fb13f3a9567fed6e
```

These hashes are informational; the acceptance rule (§4) operates on the
package hash.

---

## 4. Acceptance rule

A split variant is accepted when it passes both stages in order:

1. **Replay-identical:** the stdlib harness in `experiments/strategy-adapter/harness/`
   reproduces the retained rough-transport transcript exactly — same 6 experiments,
   5,205 steps, output `2483f08f…` — on 500 synthetic cases and the child's
   32-experiment run.

2. **Live:** the executor pipeline on the development branch:
   ```sh
   cd experiments/invention-lab/executor && cargo build -q
   B=target/debug/lab-exec; O=/tmp/split
   $B package ../../strategy-adapter/parent-split parent-split 0.1.0 method.py $O.package.json
   $B example-plan none 6 72000 60000 $O rough-transport $O.package.json 0
   $B run $O/plan.json candidate $O.package.json $O/candidate none
   (cd ../scoring && cargo run -q --bin lab-score -- score $O/plan.json candidate $O/candidate $O/candidate-score)
   $B check $O/plan.json candidate $O/candidate $O/candidate-score
   ```
   Expected: 6 experiments, 5,205 steps, output `2483f08f…`, 6/6 scored.
   This is an author-run local process, not retained evidence.

---

## 5. Finding 1: the parent's difficulty order is a no-op on rough-transport

`adapter.hardness` computes `gap + step_height` (ravine fields). On all six
rough-transport instances, the simulator sets both to `0.0`
(`crates/genesis-lab-sim/src/family.rs`: rough-transport never sets them). The
parent's 6/6 result came from its first admissible grid point, not from any
instance-ordering benefit.

Consequence for the brief: diagnosis 2 ("screen on the hardest instance") and the
A report's §1 ("the instance it ranked hardest") should note that the
screen-then-confirm loop worked, but the ranking feature was blind on this family.
The split makes this explicit: `hardness` lives in `adapter.py` where a mutator
can replace it with a real feature, independently of the search loop in
`strategy.py`.

---

## 6. Naming rule

A strategy (the `strategy.py` file or a variant of it) is called a **capability**
only after it has achieved verified wins in **two distinct families**. A win on
rough-transport alone is not sufficient.

---

## 7. What this does not cover

- No claim of recursive benefit or G0–G4 progress.
- No model calls; all evidence is local simulation.
- Mutation of strategy or adapter is future work (see
  `docs/invention-lab/proposals/iteration-algorithms.md`).
