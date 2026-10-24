![tag:innovationlab](https://img.shields.io/badge/innovationlab-3D8BD3)
![tag:hackathon](https://img.shields.io/badge/hackathon-5F43F1)

# GENESIS Organization

The front door of a GENESIS organization: an AI organization that changes its
own shape. Tell it what happened, and watch it respond: new roles, recruited
specialists, requested capabilities, handoffs and retirements, each one
validated by the organization's kernel before it takes effect.

## What you can say

- `org`: who is in the organization right now and what each role owns.
- `report <what happened>`: record evidence, for example
  `report the main hall is closed on Nov 14`. The organization's planner reads
  it and proposes the smallest valid change; the kernel decides. This agent
  relays each step as it happens, then tells you how to follow up.
- `status`: everything the organization has done since your last report.
- `methods`: installed capabilities by exact version and which role holds each.
- `lineage <capability id>`: who published each version, how it was verified, and whether this organization adopted it.
- `help`: this list.

Replies offer the next command as a button. Every button sends one of these read-only commands; none can trigger an operator action.

## What it cannot do

This agent holds only the organization's restricted agent credential. It can
record evidence and read state and events. It cannot approve or commit a
change, provision a role, publish a capability, book a venue, spend budget or
reset an operator hold. Steps labeled `DEV STUB` are local stand-ins, not live
provider integrations. Live sponsor evidence is recorded separately in the
project repository.

## Behind the conversation

GENESIS is a Rust organization kernel: roles, responsibilities, authority,
budgets, assignments with epoch fencing, a provisioning saga, an append-only
evidence log and a read-only invariant audit. Specialists are recruited as
independent Fetch.ai agents under signed contracts. Missing web abilities are
built as Apify Actors and verified against an independent oracle. Continuing
operational work lives in a persistent CREAO agent. Organization evidence is
indexed in Elastic for Agent Builder specialists.
