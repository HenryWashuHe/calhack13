# Preserved deployed-repair work in progress

This branch preserves uncommitted work from the former repository; it is not
approved runtime implementation or current product direction.

- Original repository: HenryWashuHe/genesis.
- Original base: `1795fb75ceec2465de7eaa2b3b0d1fd0073724a4`.
- Destination base: `f488eef` in genesis-history-experiment, with identical base tree.
- The original working directory remains unchanged. Changed files were copied
  byte for byte onto the matching destination base, without merging old history.
- This includes historical draft handoff/status edits. They must be reconciled
  against current main before any eventual implementation PR; they do not
  supersede the current fixed-weight method-development objective.
- Prior evidence: the server happy path only. The full revised 12-case target,
  final formatting/Clippy and independent review remain incomplete. No live
  provider or new comparison evidence is established by this preservation commit.
- Resume only under a separately claimed relevant milestone in new issue #1.

The branch is intentionally isolated from main and must remain a draft.
