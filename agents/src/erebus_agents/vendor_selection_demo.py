"""Mock rehearsal of autonomous vendor selection.

Three sellers publish the same capability at different floors. The buyer negotiates privately
with each, records what each would agree to, selects the cheapest at or below its ceiling, and
pays that one seller. Losing sellers are never paid.
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
from erebus_agents.marketplace import ServiceListing, ServiceRegistry
from erebus_agents.procurement import Catalog, Requisition, buyer_policy, review_disclosure, supplier_policy
from erebus_agents.vendor_selection import VendorOutcome, negotiate_amount, select

BUYER = "0xbuyer"
ASSET = "0xusdc"
CAPABILITY = "run-analysis"

CANDIDATES = (
    ServiceListing("svc-atlas", "0xatlas", CAPABILITY, ASSET, "job", 3, 7),
    ServiceListing("svc-aegean", "0xaegean", CAPABILITY, ASSET, "job", 7, 9),
    ServiceListing("svc-boreas", "0xboreas", CAPABILITY, ASSET, "job", 11, 14),
)


async def rehearse(capability: str, quantity: int, ceiling_unit_price: int, *, latency: float = 0.0,
                   max_rounds: int = 3, store_dir: Path,
                   candidates: tuple[ServiceListing, ...] = CANDIDATES) -> dict[str, Any]:
    requisition = Requisition(capability, quantity, ceiling_unit_price)
    store = store_dir / "erebus-mock-store.json"
    outcomes: list[VendorOutcome] = []
    for listing in candidates:
        buyer = MockErebusClient(identity=BUYER, store_path=store, latency_seconds=latency,
                                 spendable_notes=[requisition.ceiling])
        seller = MockErebusClient(identity=listing.seller, store_path=store, latency_seconds=latency,
                                  spendable_notes=[])
        amount = await negotiate_amount(
            buyer_client=buyer, seller_client=seller, buyer_address=BUYER, seller_address=listing.seller,
            token=ASSET, buyer_policy=buyer_policy(BUYER, requisition, max_rounds=max_rounds),
            seller_policy=supplier_policy(listing.seller, Catalog(capability, listing.min_unit_price), requisition,
                                          max_rounds=max_rounds),
            max_rounds=max_rounds)
        outcomes.append(VendorOutcome(listing.service_id, listing.seller, amount is not None, amount))

    selection = select(tuple(outcomes), ceiling=requisition.ceiling)
    record: dict[str, Any] = {
        "capability": capability,
        "quantity": quantity,
        "ceiling": requisition.ceiling,
        "negotiated": [{"service_id": outcome.service_id, "agreed_amount": outcome.agreed_amount}
                       for outcome in outcomes],
        "selection": {"winner": selection.winner.service_id if selection.winner else None,
                      "rejected": [{"service_id": sid, "reason": reason} for sid, reason in selection.rejected]},
    }
    if selection.winner is None:
        record["settled"] = False
        return record

    listing = next(candidate for candidate in candidates if candidate.service_id == selection.winner.service_id)
    buyer = MockErebusClient(identity=BUYER, store_path=store, latency_seconds=latency,
                             spendable_notes=[requisition.ceiling])
    winner = MockErebusClient(identity=listing.seller, store_path=store, latency_seconds=latency, spendable_notes=[])
    state = await run_negotiation(
        buyer_client=buyer, seller_client=winner, buyer_address=BUYER, seller_address=listing.seller,
        buyer_policy=buyer_policy(BUYER, requisition, max_rounds=max_rounds),
        seller_policy=supplier_policy(listing.seller, Catalog(capability, listing.min_unit_price), requisition,
                                      max_rounds=max_rounds),
        token=ASSET, max_rounds=max_rounds)
    record["settled"] = state.settlement is not None
    if state.settlement is not None:
        accepted = next(offer for offer in state.offers if offer.offer_id == state.settlement.acceptance)
        review = review_disclosure(
            DisclosedRecord(channel_id=accepted.channel_id, participants=[BUYER, listing.seller],
                            offers=state.offers, settlement=state.settlement),
            requisition)
        record["paid_amount"] = state.settlement.paid_amount
        record["audit"] = {"compliant": review.compliant, "reasons": list(review.reasons)}
    return record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capability", default=CAPABILITY)
    parser.add_argument("--quantity", type=int, default=10)
    parser.add_argument("--ceiling-unit-price", type=int, default=8)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--latency", type=float, default=0.2)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stdout)
    with tempfile.TemporaryDirectory() as tmp:
        record = asyncio.run(rehearse(args.capability, args.quantity, args.ceiling_unit_price,
                                      latency=args.latency, max_rounds=args.rounds, store_dir=Path(tmp)))
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
