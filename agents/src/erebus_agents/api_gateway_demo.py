"""Runnable rehearsal of a pay-per-call API gateway over the public `x402-exact` rail.

Each call negotiates a price, retrieves the resource once, and pays for it exactly once. The
loops run through the real production sequencing in `drive_x402_deal`, against a deterministic
offline gateway, so a dropped response or a retry can only reuse the retained permit. The
record deliberately never reports independent payment or delivery verification: for the real
rail, an independent auditor verifies payment from the disclosure grant.

This is a rehearsal, not chain evidence. For the installed rail, stand up
`erebus-access-service` and drive it with `metropolis_loop.py --profile x402-exact`. Standard
x402 is public and is not a privacy rail: accepted terms and identities are visible.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import secrets
import sys
from typing import Any

from erebus_agents.metropolis_loop import drive_x402_deal
from erebus_agents.paid_gateway import GatewayEndpoint, LocalGateway


async def _no_wait(_seconds: float) -> None:
    """The offline gateway answers immediately, so a retry needs no real back-off."""


async def rehearse(endpoint: GatewayEndpoint, calls: int, *, resource: bytes = b"paid resource body") -> dict[str, Any]:
    if calls <= 0:
        raise ValueError("calls must be positive")
    gateway = LocalGateway(endpoint, resource=resource)
    results = []
    for _ in range(calls):
        operation_ref = secrets.token_hex(32)
        record = await drive_x402_deal(gateway.buyer_session(), gateway.seller_session(), operation_ref,
                                      sleep=_no_wait)
        results.append({
            "operation_ref": operation_ref,
            "deal_commitment": record["deal_commitment"],
            "retrieval_attempts": record["retrieval_attempts"],
            "retrieval_states": record["retrieval_states"],
            "resource_sha256": record["resource_sha256"],
            "resource_bytes": record["resource_bytes"],
            "seller_reported_payment_finalized": record["seller_reported_payment_finalized"],
        })
    return {
        "capability": endpoint.capability,
        "rail": "x402-exact",
        "price_per_call": endpoint.price,
        "asset": endpoint.asset,
        "calls": results,
        "negotiate_calls": gateway.negotiate_calls,
        "permits_signed": gateway.permits_signed,
        "payments_collected": gateway.payments_collected,
        "total_billed": gateway.payments_collected * endpoint.price,
        "payment_verified": False,
        "payment_verification": "independent auditor only",
        "rails_note": "rehearsal transport; standard x402 is public, not private",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--calls", type=int, default=3, help="independent paid calls to run")
    parser.add_argument("--price", type=int, default=25, help="price per call in asset base units")
    parser.add_argument("--asset", default="0xusdc")
    parser.add_argument("--capability", default="summarize-document")
    args = parser.parse_args()
    endpoint = GatewayEndpoint("svc-gateway", args.capability, args.price, args.asset)
    record = asyncio.run(rehearse(endpoint, args.calls))
    json.dump(record, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
