"""Mock rehearsal of private SaaS metering: buy a bundle in one settlement, then spend its calls.

The buyer negotiates and settles once for a bundle of N calls to a seller's resource, then
consumes calls until the bundle is exhausted. Payment is the settlement; usage is the local
credit accounting. The two are separate: an auditor verifies the payment from the disclosure
grant, never the call count.

The bundle here is created locally and is not bound to Rust's metered issuer or its
entitlement records. On the installed rail, `erebus-access-service` serves the metered profile
and its issuer is the authority on how many calls a settlement bought.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import sys
import tempfile
from pathlib import Path
from typing import Any

from erebus_mcp.interface import DisclosedRecord
from erebus_mcp.mock_client import MockErebusClient

from erebus_agents.agent import run_negotiation
from erebus_agents.metering import BundleExhausted, CreditBundle, consume
from erebus_agents.procurement import Catalog, Requisition, buyer_policy, review_disclosure, supplier_policy

BUYER = "0xbuyer"
SELLER = "0xseller"
ASSET = "0xusdc"
ITEM = "api-call-bundle"


async def rehearse(calls: int, max_unit_price: int, floor_unit_price: int, *, consume_calls: int,
                   latency: float = 0.0, max_rounds: int = 3, store_dir: Path) -> dict[str, Any]:
    requisition = Requisition(ITEM, calls, max_unit_price)
    store = store_dir / "erebus-mock-store.json"
    buyer = MockErebusClient(identity=BUYER, store_path=store, latency_seconds=latency,
                             spendable_notes=[requisition.ceiling])
    seller = MockErebusClient(identity=SELLER, store_path=store, latency_seconds=latency, spendable_notes=[])
    state = await run_negotiation(
        buyer_client=buyer, seller_client=seller, buyer_address=BUYER, seller_address=SELLER,
        buyer_policy=buyer_policy(BUYER, requisition, max_rounds=max_rounds),
        seller_policy=supplier_policy(SELLER, Catalog(ITEM, floor_unit_price), requisition, max_rounds=max_rounds),
        token=ASSET, max_rounds=max_rounds)
    if state.settlement is None:
        return {"settled": False}

    accepted = next(offer for offer in state.offers if offer.offer_id == state.settlement.acceptance)
    review = review_disclosure(
        DisclosedRecord(channel_id=accepted.channel_id, participants=[BUYER, SELLER],
                        offers=state.offers, settlement=state.settlement),
        requisition)
    bundle = CreditBundle(f"entitlement-{accepted.deal_id}", SELLER, BUYER, calls_total=calls)
    consumed = 0
    refused: str | None = None
    for _ in range(consume_calls):
        try:
            bundle = consume(bundle, 1)
        except BundleExhausted as error:
            refused = str(error)
            break
        consumed += 1
    return {
        "settled": True,
        "agreed_amount": state.settlement.agreed_amount,
        "paid_amount": state.settlement.paid_amount,
        "bundle": bundle.to_dict(),
        "calls_consumed": consumed,
        "refused": refused,
        "audit": {"compliant": review.compliant, "reasons": list(review.reasons)},
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--calls", type=int, default=5, help="calls bought in the bundle")
    parser.add_argument("--consume", type=int, default=7, help="calls the agent tries to consume")
    parser.add_argument("--max-unit-price", type=int, default=4)
    parser.add_argument("--floor-unit-price", type=int, default=3)
    parser.add_argument("--latency", type=float, default=0.2)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stdout)
    with tempfile.TemporaryDirectory() as tmp:
        record = asyncio.run(rehearse(args.calls, args.max_unit_price, args.floor_unit_price,
                                      consume_calls=args.consume, latency=args.latency, store_dir=Path(tmp)))
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
