"""Buy a listed capability over the real Metropolis loop: search, negotiate, settle, deliver.

A buyer searches a `ServiceRegistry` for a capability and picks the cheapest listing whose
advertised floor is inside its ceiling. The buyer operator's route file maps that listing to the
configured servers for that seller, and the purchase runs through `metropolis_loop.run_over_mcp`:
both sides negotiate, the buyer settles at most once, and recovery observes until the payment is
finalized. The agent supplies only the operation ID. Price, keys and settlement mode are fixed by
the operators' files, and the buyer's `maximum_price` is what Rust enforces; the advertised band
only chose which seller to approach.

When the buyer's server is configured with access, the paid resource is retrieved and the
delivery is recorded as DELIVERED with its verified hash. Otherwise delivery stays PENDING for the
operator to record. Delivery is an off-chain observation, never settlement evidence. Public-bound
settlement exposes the accepted terms and parties; it is not private.

Reuse the same `--operation` after any interruption: it is the recovery handle, and a new one
would be a new deal.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import re
import sys
from pathlib import Path
from typing import Any

from erebus_agents.marketplace import (
    DeliveryRecord,
    DeliveryStatus,
    PurchaseRoute,
    ServiceListing,
    ServiceRegistry,
    load_routes,
    mark_delivered,
)
from erebus_agents.metropolis_loop import _harness_error, run_over_mcp, server_params

_OPERATION = re.compile(r"[a-f0-9]{64}")


def select_listing(registry: ServiceRegistry, routes: dict[str, PurchaseRoute], capability: str,
                   ceiling_unit_price: int) -> tuple[ServiceListing, PurchaseRoute] | None:
    """The cheapest listing inside the ceiling that this buyer's operator has wired a route to."""
    for listing in registry.search(capability, max_unit_price=ceiling_unit_price):
        route = routes.get(listing.service_id)
        if route is not None:
            return listing, route
    return None


def purchase_record(listing: ServiceListing, deal: dict[str, Any]) -> dict[str, Any]:
    """Project a finalized `drive_deal` record into the marketplace's purchase record."""
    delivery = DeliveryRecord(deal["operation_ref"], listing.service_id, DeliveryStatus.PENDING)
    if "delivery" in deal:
        delivery = mark_delivered(delivery, deal["delivery"]["resource_sha256"])
    return {
        "listing": listing.to_dict(),
        "operation_ref": deal["operation_ref"],
        "deal_commitment": deal["deal_commitment"],
        "payment_verified": deal["payment_verified"] is True,
        "stage": deal.get("stage"),
        "settle_calls": deal["settle_calls"],
        "recover_calls": deal["recover_calls"],
        "delivery": {"status": delivery.status.value, "artifact_sha256": delivery.artifact_sha256,
                     "is_settlement_evidence": False},
        "stages_ms": deal.get("stages_ms"),
    }


async def purchase(listing: ServiceListing, route: PurchaseRoute, operation_ref: str, *,
                   server_command: str | None = None, negotiation_cli: str | None = None,
                   payment_cli: str | None = None, max_polls: int = 120) -> dict[str, Any]:
    if not _OPERATION.fullmatch(operation_ref) or operation_ref == "0" * 64:
        raise ValueError("operation_ref must be 64 nonzero lowercase hex digits")
    access = json.loads(Path(route.buyer_access_file).read_text()) if route.buyer_access_file else None
    buyer = server_params(sys.executable, route.buyer_negotiation_config, payment_config=route.buyer_payment_config,
                          access=access, server_command=server_command, negotiation_cli=negotiation_cli,
                          payment_cli=payment_cli)
    seller = server_params(sys.executable, route.seller_negotiation_config, negotiation_cli=negotiation_cli,
                           server_command=server_command)
    deal = await run_over_mcp(buyer, seller, operation_ref, max_polls=max_polls)
    return purchase_record(listing, deal)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, required=True, help="the ServiceRegistry JSON file")
    parser.add_argument("--routes", type=Path, required=True, help="the buyer operator's route file")
    parser.add_argument("--capability", required=True)
    parser.add_argument("--ceiling-unit-price", type=int, required=True, help="the buyer's maximum advertised unit price")
    parser.add_argument("--operation", required=True, help="64-hex operation ID; reuse it after any interruption")
    parser.add_argument("--server-command", help="installed erebus-mcp-server; default: this Python's module")
    parser.add_argument("--negotiation-cli")
    parser.add_argument("--payment-cli")
    parser.add_argument("--max-polls", type=int, default=120)
    args = parser.parse_args()
    selected = select_listing(ServiceRegistry.load(args.registry), load_routes(args.routes), args.capability,
                              args.ceiling_unit_price)
    if selected is None:
        print(json.dumps({"matched": False, "capability": args.capability,
                          "ceiling_unit_price": args.ceiling_unit_price}))
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
        print(json.dumps({"matched": True, "service_id": listing.service_id, "operation_ref": args.operation,
                          "payment_verified": False, "error": str(found)}))
        raise SystemExit(1) from None
    print(json.dumps({"matched": True, **record}, indent=2))


if __name__ == "__main__":
    main()
