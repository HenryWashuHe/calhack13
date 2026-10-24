# genesis-lab-exec-lines/1

A development method is one program started by the executor. It talks to the
executor with one JSON object per line: requests on its standard output,
replies on its standard input, in order, one reply per request. Nothing else
may be written to standard output. The method has no network and no other way
to run the simulator; every simulation goes through `experiment`.

## Requests

```json
{"op":"instances"}
{"op":"experiment","instance":"<instance id>","design":{...design...}}
{"op":"submit","design":{...design...}}
```

- `instances` lists the public development instances of this run and the
  remaining simulation steps.
- `experiment` simulates one design on one listed instance. Each call reserves
  that instance's full episode (`max_steps`) from the run's simulation grant
  and is charged the steps actually simulated. When a full episode no longer
  fits, the call is refused.
- `submit` retains the run's single output design. Only the first valid
  submission is kept; later ones are refused.

A request line longer than 1 MiB fails the run. The run is stopped at its wall
time limit; a submission made before that is kept.

## Replies

Every reply has `"ok"`. A refused request has `"ok": false` and `"refused"`
with a reason; it costs nothing.

`instances`:

```json
{"ok":true,"remaining_steps":72000,
 "instances":[{"id":"ravine-crossing/1/0","max_steps":2400,"world":{...}}]}
```

`world` describes the task instance, including `rules.zone`
`[x0, y0, x1, y1]` (every body must start wholly inside it) and `rules.budget`
(maximum total part cost).

`experiment`:

```json
{"ok":true,"index":3,"remaining_steps":64800,"outcome":"scored",
 "metrics":{"success":false,"progress":0.42,"cost":3.1,"joints_broken":0,
            "steps_to_target":null},
 "steps":2400,"trace_sha256":"...","reason":null}
```

`outcome` is `scored` (the simulation ran; `metrics.success` says whether the
design met the task), `invalid` (the design failed validation and wasn't
simulated, 0 steps; `reason` says why), `failed` (the simulation diverged) or
`unknown` (the simulator stopped without a result; further experiments are
refused).

`submit`:

```json
{"ok":true,"output":"<sha256 of the normalized design>"}
```

## Designs

Designs use design language 1, described in the separate design-language
document. The executor normalizes a parsed design before storing it.

## Ending

Exit after submitting. Exiting with status 0 and a submission records the run
as having generated output. A method that doesn't submit, exits non-zero, or
runs past its wall time is recorded accordingly; nothing is retried.
