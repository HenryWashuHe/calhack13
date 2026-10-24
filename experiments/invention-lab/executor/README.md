# Invention-lab executor v0 (experimental)

Henry's Claude, issue #1 executor lane. Runs **one frozen trial slot** from a
#28 `TrialPlan`. Every experiment goes through Codex's #32 `ExperimentLedger`
against the in-process evaluator (`../evaluator`), and the run ends with a #18
`ExecutionReceipt`. Standalone crate, not a root workspace member.

```sh
cd experiments/invention-lab/executor
cargo test                       # the sandbox test runs only as root
B=target/debug/lab-exec
$B example-plan namespaces 6 72000 60000 /var/tmp/lab/plan.json
$B run /var/tmp/lab/plan.json candidate methods/random-search/method.json /var/tmp/lab/candidate namespaces
$B run /var/tmp/lab/plan.json control   methods/fixed-cart/method.json    /var/tmp/lab/control   namespaces
$B check /var/tmp/lab/plan.json candidate /var/tmp/lab/candidate
```

## What a run does

1. **Before spawning anything** it checks three identities against the plan:
   - the method manifest-plus-entry digest equals the slot's `method`;
   - the executor environment digest (its sources, sandbox mode and
     interpreter version) equals `development_environment`;
   - the linked evaluator's content digest equals `context.evaluator`.

   Any mismatch, or any dependency, refuses with nothing written. A slot
   directory that already holds state is never started again.
2. The method talks JSON lines on stdin/stdout: `instances`, `experiment`,
   `submit`.
3. Each `experiment` goes through these steps in order:
   1. Check the instance: it must be one of the plan's public instances, and
      its id and digest are re-resolved.
   2. `reserve` a full episode.
   3. Durable write of the ledger (tmp file, fsync, rename, fsync of the
      directory).
   4. Run the evaluator.
   5. Store the result artifact.
   6. `settle`.
   7. Durable write again, then reply.

   Settlement mapping: evaluator `Unknown` becomes unknown usage, which blocks
   further calls; `Failed` becomes a known, charged failure. Refusals (unknown
   instance, spent grant, bad request) are replies, not records.
4. `submit` keeps at most one output: the normalized design bytes, stored
   before the reply.
5. At exit, at the wall-time kill (grant minus 250 ms), or after an oversized
   request line (more than 1 MiB): `seal`, write `transcript.json` (exact
   `transcript_json` bytes), write `receipt.json`.
6. `recover` handles an executor that stopped mid-run. A pending call is
   settled `Unknown` and is never resubmitted; the receipt outcome is
   `unknown`; the method isn't restarted.

`check` runs `validate_execution` and `assess_trials` on the stored records.

## Isolation, exactly

`namespaces` mode (root only):

- **Enforced:**
  - new network, mount and PID namespaces;
  - a read-only root filesystem;
  - private tmpfs over `/home`, `/root`, `/tmp`, `/var/tmp`, `/dev/shm` and
    `/run`;
  - the method runs as `nobody` with `no_new_privs`;
  - the whole PID namespace is killed at the limit.

  The method can't see the evaluator, its sources, the slot state or other
  runs, can't reach the network, and nothing it writes survives the run. With
  no network and no model inside, model calls and tokens are recorded as
  measured zero.
- **Not enforced:** reads of the rest of the read-only system; any syscall
  filter; CPU or memory quotas beyond the wall time and the tmpfs sizes. A
  method can compute its own physics inside its wall time, so
  `simulation_steps` meters evaluator calls only.

`none` mode has no isolation, so model calls and tokens are recorded as
`unknown`. The plan pins the mode through the environment digest.

## Seed methods are controls

`methods/random-search` (random search over wheeled carts) and
`methods/fixed-cart` (one fixed design, no experiments) are hand-written and
labeled `control`. They exercise the executor end to end. They are not
generated or improved methods and say nothing about recursive benefit.

## Not in v0

- No trusted scoring adapter: `assess_trials` shows scoring as `Missing`.
- No comparison runner and no hidden-set reveal.
- No model access or dependencies.
- The store is a local directory: it doesn't stop someone with write access
  from rolling it back, and it doesn't authenticate records.
