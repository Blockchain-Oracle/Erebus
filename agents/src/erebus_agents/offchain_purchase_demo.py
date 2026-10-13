"""One-off off-chain purchases -- compute, a dataset, or an API response -- over the real loop.

These are the same shape as a marketplace purchase: one buyer, one seller, one settlement, an
off-chain deliverable. So this is the marketplace purchase (`marketplace_demo.purchase`) with the
capability named by the kind of thing bought. It shows that "buy compute" and "buy a dataset"
need no new mechanism, not to add one. The registry, the buyer operator's routes and the
operation ID are the same inputs `marketplace_demo.py` takes.
"""

from __future__ import annotations

import argparse
import asyncio
import json
from pathlib import Path

from erebus_agents.marketplace import ServiceRegistry, load_routes
from erebus_agents.marketplace_demo import purchase, select_listing
from erebus_agents.metropolis_loop import _harness_error

KINDS = {"compute": "run-model", "dataset": "purchase-dataset", "api": "call-api"}


def capability_for(kind: str) -> str:
    if kind not in KINDS:
        raise ValueError(f"kind must be one of {sorted(KINDS)}")
    return KINDS[kind]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=sorted(KINDS), required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--routes", type=Path, required=True)
    parser.add_argument("--ceiling-unit-price", type=int, required=True)
    parser.add_argument("--operation", required=True, help="64-hex operation ID; reuse it after any interruption")
    parser.add_argument("--server-command")
    parser.add_argument("--negotiation-cli")
    parser.add_argument("--payment-cli")
    parser.add_argument("--max-polls", type=int, default=120)
    args = parser.parse_args()
    capability = capability_for(args.kind)
    selected = select_listing(ServiceRegistry.load(args.registry), load_routes(args.routes), capability,
                              args.ceiling_unit_price)
    if selected is None:
        print(json.dumps({"kind": args.kind, "matched": False, "capability": capability}))
        raise SystemExit(1)
    listing, route = selected
    try:
        record = asyncio.run(purchase(listing, route, args.operation, server_command=args.server_command,
                                      negotiation_cli=args.negotiation_cli, payment_cli=args.payment_cli,
                                      max_polls=args.max_polls))
    except BaseException as error:
        found = _harness_error(error)
        if found is None:
            raise
        print(json.dumps({"kind": args.kind, "operation_ref": args.operation, "payment_verified": False,
                          "error": str(found)}))
        raise SystemExit(1) from None
    print(json.dumps({"kind": args.kind, "matched": True, **record}, indent=2))


if __name__ == "__main__":
    main()
