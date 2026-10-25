# Method adoption, authorization and runs

How a receiving organization goes from a selected method to one governed run
of it. Operator routes on the organization server; every step re-reads current
state under the kernel lock. Nothing on the server builds a plan, runs a method
or scores an output.

## 1. Adopt: `POST /method-adoptions`

One explicit decision about one exact published version, bound to the
requirement, its owner role and responsibility, and the current owner grant.
`adopt` recomputes acceptance from the receiver's own retained trial, which
must be under the plan the receiver pinned for that requirement and publisher.
`decline` records the choice. Only the requirement's owner role can adopt.

## 2. Authorize: `POST /method-invocations/authorize`

Answers whether this role may invoke this exact version for this
responsibility now. It holds only while:
- the adoption's requirement, mission, authority and grant are unchanged;
- the receiver's current pin still names the same publisher and plan;
- the receiver still accepts the trial's environment, evaluator and dependencies.

A change to any of these fences the adoption. It is never rewritten; a new
adoption is the way back.

## 3. Run: `/method-runs`

| Step | Route | Effect |
| --- | --- | --- |
| Prepare | `POST /method-runs` `{client_request_id, request, plan}` | Authorizes, checks the proposed plan, freezes it as `prepared`. Idempotent by key. |
| Start | `POST /method-runs/{id}/start` | `prepared → started` once if the frozen grant is still current, else `prepared → fenced` (final, nothing runs). |
| Receipt | `POST /method-runs/{id}/receipt` | The execution receipt, checked against the frozen plan's binding. `completed`, or `completed_after_fence` if the grant changed during the run. Any outcome, `unknown` included, is final. |
| Read | `GET /method-runs/{id}` | The record. |

A proposed plan is accepted only if all of these hold:
- It is a one-slot `development-trial-plan/1` with purpose `invocation`, naming
  exactly the current grant (adoption, requirement, role, responsibility,
  assignment, epoch).
- The slot runs the adopted reference for this organization, with the
  dependencies the method was accepted with.
- Its public task, evaluator and development environment are the ones in the
  acceptance evidence.
- Its model settings equal the accepted plan's.
- Its development and scoring grants are no larger than the accepted plan's.

- Its plan, run, attempt and scoring ids are used by no other run, and its
  run, attempt and scoring ids differ from the accepted trial's.
- Its frozen record leaves 64 KiB for the receipt, so a run that starts can
  always be completed.

Its instances are the operator's choice; they are public.

A recorded run also carries `budget`: the receipt's usage against the frozen
development grant (`within_limits`, `unknown` or `exceeded` with the resource).
An overrun is recorded, never hidden, and authorizes nothing further.

There is no retry. A started run whose runner is lost stays `started`; its
state is recovered with `lab-exec recover` and the receipt posted by hand.

`experiments/invention-lab/scoring/tools/invoke.py` drives prepare, start,
`lab-exec run` on the server's frozen plan, and the receipt post. The operator
token comes from `$GENESIS_OPERATOR_TOKEN`.

## Not covered

- Scoring the run's output, and attaching the result to the responsibility as
  work done. A run record is evidence that the method ran under authority, not
  that the responsibility is satisfied.
- Re-acceptance after a policy change, other than adopting again.
