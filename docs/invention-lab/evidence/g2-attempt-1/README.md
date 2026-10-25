# `g2-attempt-1` result (October 25, 2026)

[Protocol](../../protocols/g2-attempt-1.md), frozen at `37706b1` and
predeclared on [issue #1](https://github.com/HenryWashuHe/genesis-history-experiment/issues/1#issuecomment-6395793768)
before it ran. Ran once, 60 s, as root, sandbox namespaces, pinned Python
3.14.0. Four live planner requests in total (`claude-opus-5-5`, one per
instance, no repair turns or retries). One host, loopback, dev-stub
providers, operator-run. The independent review stood in for Codex and Eric,
who were paused.

## Results

| instance | planner owner | B's pinned trial vs fixed cart | reported: vs random | receiver decision | invocation on fresh seeds |
|---|---|---|---|---|---|
| B1-parent | existing Lab lead | **12/12 vs 4/12, accepted** | 12 vs 2 | **adopted** | completed, **10/12**, attached |
| B1-same-cart | existing Lab lead | 7/12 vs 7/12, rejected | 7 vs 4 | **not_applicable** | none |
| B2-parent | existing Lab lead | **11/12 vs 7/12, accepted** | 11 vs 9 | **adopted** | completed, **11/12**, attached |
| B2-same-cart | existing Lab lead | 8/12 vs 8/12, rejected | 8 vs 7 | **not_applicable** | none |

Controls, in every instance: a bearer token B never issued got 401; the
mismatched grant (the lead with its unrelated "Coordinate the lab"
responsibility) got 409 `method_authority`. Shared: B's reader token
publishing as A was refused; a token A never issued got 401 on A; A's state
held no requirement at the end.

## Against the predeclared expectations

- `parent` adopted in both B instances, invoked and scored: **as expected.**
  A's exact model-generated method, developed for ravine-crossing, was found
  by B on the registry, checked by B on B's own rough-transport task, adopted
  by B's owner and used under B's grant, with the result attached to B's
  requirement, responsibility, role and adoption.
- `same-cart` refused by the receiver gate (`not_applicable`) in both: **as
  expected**, and by construction (it ties the fixed cart).
- B1 assigned to the existing lead: **as expected.**
- B2 created a specialist or used a role holding `method.develop`: **not met.**
  B2's planner gave the work to its existing lead, whose charter is "Own the
  lab and run rough-transport deliveries" and who holds only
  `mission.coordinate`. Its rationale reads the charter as covering
  rough-transport work and doesn't mention scopes. The server then let that
  role adopt the method and run it, since nothing requires `method.develop`
  for adoption or invocation.

## What this shows, and what it doesn't

It is the first run in which a B organization, with a live planner, took an
exact method A's model generated, checked it against its own baseline on a
different task, adopted it and used it with a scored outcome, while its
receiver refused an offer that didn't beat that baseline. That's the
positive path of the G2 row, exercised end to end once per B variant.

It is not independent-organization evidence (one host, one operator), the
receipts stay operator-reported, and four planner calls don't measure
planner judgment. The `same-cart` refusal is a plumbing check, not a test of
B's judgment.

The B2 outcome is the finding worth acting on: **organizational authority
isn't enforced on method adoption.** The ownership decision didn't follow
the change in B's organization, and nothing downstream caught it. Either the
kernel should refuse an `AssignResponsibility` for a method-development
requirement to a role without `method.develop`, or adoption/invocation
should require it of the grant's role. Until one of those exists, "justified
organizational responsibility" in G2 rests on the planner alone.

## Files

`run/summary.json` (all four instances); per instance: `mission.json`,
`intake.json`, `planner-prompt.txt` (7,501–7,512 bytes), `planner-once.*`,
`organization-after-planner.json`, `trial/` (receive.py output with scorer
artifacts), `receiver.json`, `refused.json`, `decision.json`, and for
adoptions `authorization.json`, `proposed-*.json`, `run/` and
`run-record.json`. Process logs are included; SQLite files are not.
`SHA256SUMS` covers the committed files; the API key appears in none of them.
