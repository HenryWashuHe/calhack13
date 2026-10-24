# Apify sponsor announcement and demo implications

The later [CREAO sponsor notes](CREAO_SPONSOR_NOTES.md) complement this record;
both sponsors need meaningful mission work, with their evidence kept distinct.

Source: Petros Hong's Cal Hacks announcement, supplied by the team in chat on
October 21, 2026. These are notes from that announcement, not independently
confirmed competition rules or a guarantee of coupon availability.

## Announced support

- Projects must use Apify meaningfully; examples include web data, workflow
  automation and powering an AI agent.
- General prizes: $2,000 / $1,000 / $800 / $400 / $400.
- Separate category: **Best Actor built with AI Chat — $400**. Total announced
  prizes, including this category: $5,000.
- Announced credits: $200 via the [Cal Hacks signup link](https://console.apify.com/sign-up?couponId=APIFY_CAL_HACKS).
  Existing users were directed to Billing → Subscription → Promo Code and
  `APIFY_CAL_HACKS`; paying members were directed to contact the sponsor.
- Announced access options: MCP, CLI and API. MCP endpoint:
  `https://mcp.apify.com`. Workshop timing was still TBD in the supplied message.

No coupon has been redeemed or account/billing setting changed by this update.

## GENESIS demo decision

Keep the existing venue-relocation mission. A closure exposes a missing source of
venue availability, causing both a new specialist responsibility and a new Apify
Actor. Show the Actor build, its real execution, independent output checks and the
resulting feasible relocation plan. Organization B then discovers the approved
capability and runs its pinned build on different input with its own acceptance
checks. This makes Apify necessary to the organizational evolution story.

Use the implemented Rust REST adapters for backend creation/build/execution. The
announcement does not make MCP a requirement, so an extra MCP layer is unnecessary
for prize positioning. Preserve the source/build/run/dataset identities and the
independent verification evidence in the judge-facing demonstration. Do not present
loopback tests as cloud execution or a hosted synthetic portal as live venue data.

Track the AI Chat category separately. GENESIS generating source through a model
is not, by itself, evidence that the Actor was built with Apify AI Chat. If entering
that category, confirm its rules with the sponsor and retain the actual AI Chat
build provenance. Do not change the core product direction just to fit the bonus.

## Remaining evidence

The announcement does not close the live integration gate: create/build/run the
Actor in an authenticated Apify account, collect native IDs and independently
check its output. Current implementation and recovery details are in
[FORGE_INTEGRATION.md](FORGE_INTEGRATION.md) and
[CAPABILITY_EXECUTION.md](CAPABILITY_EXECUTION.md).
