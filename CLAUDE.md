# GENESIS agent coordination

Claude currently owns the kernel and UI. Codex's capability acceptance core and
Apify execution adapter were merged in [PR #1](https://github.com/HenryWashuHe/genesis/pull/1).
Codex is implementing source creation/builds, the checkpointed forge and Actor
runtime on `codex/capability-forge` ([PR #2](https://github.com/HenryWashuHe/genesis/pull/2)).
Its paths are `crates/genesis-{verification,apify,forge}`, `actors/venue-extractor-template`,
and associated capability scripts/docs. These components return receipts and
verified drafts; Claude's kernel retains publication, installation and role authority.

**The user authorizes Codex to take over kernel implementation if Claude runs out
of credits.** This is a fallback, not an ownership transfer already in effect.
UI ownership remains with Claude unless separately reassigned.

See [the handoff record](docs/AGENT_HANDOFF.md) for current ownership and the
information to leave at a checkpoint. Commit and push often: the agents work on
different machines, so unpushed changes are not visible to the other agent.
