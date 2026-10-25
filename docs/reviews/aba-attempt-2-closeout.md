# ABA attempt 2 driver correction

This corrects the prospective ignored driver in [PR #250](https://github.com/HenryWashuHe/genesis-history-experiment/pull/250),
reviewed at `0db2d320017f83105f87703b7cc5c71ef7d88033`. It does not run or
reinterpret an experiment. Historical evidence, manifests and shared receiving,
executor and scoring code are unchanged.

The corrected driver:

- Waits for the exact observation's unambiguous `ASSIGNED` requirement and
  matching open responsibility. A pending requirement is insufficient. Missing
  assignment and a different selected owner are retained stop outcomes.
- Copies the declared spec and validated canonical package bytes into a local
  snapshot. Only paths change in the derived spec; original bytes remain retained.
  Before B's trial, the driver prepares A's plan with the existing `example-plan`
  interface. After A's assignment, it installs that exact plan reference in A's
  receiver and restarts the server **before** A's receiving trial. The returned
  full typed plan must equal the prepared plan before adoption can continue.
- Retains and assesses both candidate and control execution/scoring evidence with
  the existing core `assess_process_slot` rule. A generic Accepted verdict is
  insufficient: Failed, Interrupted, Unknown, missing or mismatched process
  evidence cannot authorize B publication or A adoption. Raw failed evidence is
  retained; the driver records the additional eligibility assessment separately.

These are future driver behaviors. The snapshot assumes operator-controlled local
storage; it is not a filesystem-confinement or externally authenticated chronology
claim. The existing ignored driver still requires explicit live-run authorization.
This change does not establish independent organization behavior or close G0–G4.

Driver SHA-256 before this correction:
`f450d9e843eeec02f4a6e94f7dcaa0c3467d15b43c4ee88d359a8e6594b15ec2`.
Corrected driver SHA-256:
`b2a1411b78a50183acd9f92abfb100a3bf1c3f0b65e0f3482501934dd54db640`.

Validation was limited to six pure retained-record/local-file guards in
`review_aba_attempt_2` (`policy_` filter; the imported ignored driver was filtered
out), compile-only checking of `aba_attempt_2`, strict Clippy for the guard target,
Rust formatting and whitespace checks. No driver, generated method, provider,
evaluator, scorer or namespace was executed. The guards include a generic
Accepted verdict with Failed/Interrupted receipts, the valid retained-record
control, exact-plan reconstruction, snapshot preservation and source-order checks.
