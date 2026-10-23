# GENESIS agent coordination

Claude currently owns the kernel and UI. Codex has implemented the capability
acceptance core and Apify execution adapter on `codex/capability-verification`
([PR #1](https://github.com/HenryWashuHe/genesis/pull/1)).

**The user authorizes Codex to take over kernel implementation if Claude runs out
of credits.** This is a fallback, not an ownership transfer already in effect.
UI ownership remains with Claude unless separately reassigned.

See [the handoff record](docs/AGENT_HANDOFF.md) for current ownership and the
information to leave at a checkpoint. Commit and push often: the agents work on
different machines, so unpushed changes are not visible to the other agent.
