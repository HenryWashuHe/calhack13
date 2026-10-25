# Invention-lab executor v0 (experimental)

Henry's Claude, issue #1 executor lane. Runs **one frozen trial slot** from a
#28 `TrialPlan`. Every experiment goes through Codex's #32 `ExperimentLedger`
against the in-process evaluator (`../evaluator`), and the run ends with a #18
`ExecutionReceipt`. Standalone crate, not a root workspace member.

```sh
cd experiments/invention-lab/executor
cargo test                       # the sandbox test runs only as root
B=target/debug/lab-exec
$B example-plan namespaces 6 72000 60000 /var/tmp/lab   # writes plan + two packages
$B run /var/tmp/lab/plan.json candidate /var/tmp/lab/candidate.package.json /var/tmp/lab/candidate namespaces
$B run /var/tmp/lab/plan.json control   /var/tmp/lab/control.package.json   /var/tmp/lab/control   namespaces
(cd ../scoring && cargo run -q --bin lab-score -- /var/tmp/lab/plan.json candidate /var/tmp/lab/candidate /var/tmp/lab/candidate-score)
$B check /var/tmp/lab/plan.json candidate /var/tmp/lab/candidate /var/tmp/lab/candidate-score
```

Methods are #36 `development-method-source/1` packages (`lab-exec package`
builds one from a source directory). The host selects the runtime (`python3`
at its exact version) and the line protocol; a package naming anything else
is refused by `MethodPackage::load_for`.

## What a run does

1. **Before spawning anything** it checks identities against the plan:
   - the exact package bytes load for the slot's `method` (digest, canonical
     bytes, labels, dependencies) under this host's runtime and protocol;
   - the executor environment digest (its sources, sandbox mode and
     interpreter version) equals `development_environment`;
   - the linked evaluator's content digest equals `context.evaluator`.

   Any mismatch, or any dependency, refuses with nothing written. The runner
   then takes the slot's exclusive claim (a lock on `slot.lock`) and holds it
   through finalization; a concurrent runner, or a slot that already holds
   state, gets `AlreadyStarted` and is never started again.
2. The method talks JSON lines on stdin/stdout: `instances`, `experiment`,
   `submit`. Replies go through a bounded queue (64) to a writer thread; a
   method that keeps sending without reading fails the run instead of blocking
   it, and the deadline is checked before every queued request.
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
   request line (more than 1 MiB), the child is stopped first. Then `seal`
   (after which the session accepts nothing), write `transcript.json` (exact
   `transcript_json` bytes), persist the sealed ledger and run state, and only
   then write `receipt.json`.
6. `recover` handles an executor that stopped mid-run or mid-finalization. A
   pending call is settled `Unknown` and is never resubmitted; the receipt
   outcome is `unknown`; the method isn't restarted.

`check` runs `validate_execution` and `assess_trials` on the stored records.

Trusted acceptance scoring lives in the separate `../scoring` crate.

## Isolation, exactly

`namespaces` mode (root only):

- **Enforced:**
  - new network, IPC, mount and PID namespaces, then a chroot into a minimal
    method root built for this run. Read-only, it holds only the interpreter
    and its standard library (any `site-packages`/`dist-packages` inside it
    masked empty), `sh`, `prlimit`, `setpriv`, the shared libraries those and
    the stdlib extension modules link, `/etc/ld.so.cache` and
    `/dev/{null,zero,random,urandom}`. There is no `/proc`, `/home`, `/run`,
    `/var`, host `/tmp` or other mount in it, and setup refuses to start if
    any socket file is inside it;
  - the only writable place is a private tmpfs `/tmp`, with the package's
    source files in `/tmp/work`;
  - only stdin, stdout and stderr cross into the namespace: every other
    descriptor the executor holds (say, a directory or socket its launcher
    left open) is marked close-on-exec first, so none can lead out of the
    chroot. This requires Linux `close_range(CLOSE_RANGE_CLOEXEC)` support
    (Linux 5.11+) and permission to call it. Failure refuses startup; there
    is no partial walk based on the launcher's current descriptor limit,
    which may be lower than descriptors it already holds. `namespaces` is
    unsupported on non-Linux hosts; `none` mode is unchanged;
  - the method runs as `nobody` with `no_new_privs`, so it can't leave the
    chroot;
  - at the deadline or after the outcome, the executor attempts to SIGKILL
    the namespace's first process with kill(2). Whole-tree termination
    evidence requires a confirming stop outcome and successful launcher
    reap; a launcher-only fallback remains unconfirmed and cannot authorize
    hidden scoring;
  - resource limits set with prlimit before the privilege drop: 1 GiB address
    space, 64 processes for `nobody`, 64 MiB per written file, 256 open files.
- **Model use is measured zero in this mode.** With no network, no socket file
  or IPC peer, and no files beyond the interpreter image and the method's own
  source, no external model or model service is reachable. Whatever the
  method computes itself is its own work.
- **Not enforced:** any syscall filter; a CPU quota beyond the wall time. The
  process limit is per user, so concurrent runs share it. A method can
  compute its own physics inside its wall time, so `simulation_steps` meters
  evaluator calls only.

Running the root-gated tests: the pinned interpreter must live under `/usr`,
but not under `/usr/local`, `/root` or `/opt`. The method root exposes the
interpreter image's directory chain, and the probes assert those paths stay
invisible, so an interpreter installed there (for example a `uv` build under
`/root/.local`) makes them fail even though the sandbox is working.

`none` mode has no isolation at all, and model use stays `unknown`. The plan
pins the mode through the environment digest.

## Seed methods are controls

`methods/random-search` (random search over wheeled carts) and
`methods/fixed-cart` (one fixed design, no experiments) are hand-written and
labeled `control`. They exercise the executor end to end. They are not
generated or improved methods and say nothing about recursive benefit.

## Not in v0

- No comparison runner, hidden-set scoring or reveal.
- No criterion evaluation or adoption decision.
- No model access or dependencies.
- The store is a local directory: it doesn't stop someone with write access
  from rolling it back, and it doesn't authenticate records.
