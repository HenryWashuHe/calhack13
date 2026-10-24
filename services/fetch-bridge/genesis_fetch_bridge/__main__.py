"""Private bridge CLI. Secrets are environment-only and never persisted."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from aiohttp import web
from uagents_core.identity import Identity

from .bridge import Bridge
from .wire import Invalid, decode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser(
        "identity",
        help="Print the deterministic public identity; no network or DB writes",
    )
    serve = commands.add_parser("serve")
    serve.add_argument("--db", required=True)
    serve.add_argument(
        "--peers",
        required=True,
        help="JSON map of fixed agent addresses to /submit URLs",
    )
    serve.add_argument("--port", type=int, required=True)
    serve.add_argument("--host", choices=["127.0.0.1", "::1"], default="127.0.0.1")
    serve.add_argument(
        "--specialist",
        action="store_true",
        help="Enable the synthetic advisory-only specialist",
    )
    serve.add_argument("--max-records", type=int, default=10000)
    serve.add_argument("--max-bytes", type=int, default=64 * 1024 * 1024)
    args = parser.parse_args()
    try:
        seed = os.environ.get("GENESIS_FETCH_SEED", "")
        if not 32 <= len(seed.encode()) <= 4096:
            raise Invalid("GENESIS_FETCH_SEED must contain 32..4096 bytes")
        if args.command == "identity":
            print(Identity.from_seed(seed, 0).address)
            return
        if not 1 <= args.port <= 65535:
            raise Invalid("port must be in 1..65535")
        with Path(args.peers).open("rb") as stream:
            peers = decode(stream.read(64 * 1024 + 1))
        bridge = Bridge(
            seed=seed,
            token=os.environ.get("GENESIS_FETCH_BRIDGE_TOKEN", ""),
            db=args.db,
            peers=peers,
            specialist=args.specialist,
            max_records=args.max_records,
            max_bytes=args.max_bytes,
        )
        web.run_app(
            bridge.app,
            host=args.host,
            port=args.port,
            access_log=None,
            print=None,
            auto_decompress=False,
        )
    except (Invalid, OSError):
        # OSError can include a pathname but never a seed or token; keep startup output generic.
        print(
            "Bridge configuration or storage is invalid. Check seed/token presence, peer file, DB identity and local port.",
            file=sys.stderr,
        )
        raise SystemExit(2) from None


if __name__ == "__main__":
    main()
