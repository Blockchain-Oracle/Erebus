"""Mock rehearsal of the marketplace loop: search a registry, negotiate, settle, deliver, audit.

A buyer searches a `ServiceRegistry` for a capability, picks the cheapest listing inside its
ceiling, negotiates against that seller's reserve on the mock client, pays once, records the
off-chain delivery, and has the deal reviewed against the buyer's policy.

This is a rehearsal, not chain evidence. `deal_ref` is a local digest, and the delivery record
is an operator observation the auditor may read but cannot treat as settlement proof.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import logging
import sys
import tempfile
from pathlib import Path
from typing import Any

from erebus_mcp.interface import DisclosedRecord
from erebus_mcp.mock_client import MockErebusClient

from erebus_agents.agent import run_negotiation
from erebus_agents.marketplace import (
    DeliveryRecord,
    DeliveryStatus,
    ServiceListing,
    ServiceRegistry,
    mark_delivered,
)
from erebus_agents.procurement import Catalog, Requisition, buyer_policy, review_disclosure, supplier_policy

BUYER = "0xbuyer"
AUDITOR = "0xauditor"
ASSET = "0xusdc"

DEFAULT_LISTINGS = (
    ServiceListing("svc-atlas", "0xatlas", "summarize-documents", ASSET, "page", 3, 7),
    ServiceListing("svc-aegean", "0xaegean", "summarize-documents", ASSET, "page", 4, 9),
    ServiceListing("svc-boreas", "0xboreas", "summarize-documents", ASSET, "page", 6, 12),
)


def seed_registry(listings: tuple[ServiceListing, ...] = DEFAULT_LISTINGS) -> ServiceRegistry:
    registry = ServiceRegistry()
    for listing in listings:
        registry.publish(listing)
    return registry


async def rehearse(capability: str, ceiling_unit_price: int, quantity: int, *,
                   registry: ServiceRegistry, latency: float = 0.0, max_rounds: int = 3,
                   store_dir: Path, resource: bytes = b"the summarized document") -> dict[str, Any]:
    listing = registry.best(capability, max_unit_price=ceiling_unit_price)
    if listing is None:
        return {"matched": False, "capability": capability, "ceiling_unit_price": ceiling_unit_price}

    requisition = Requisition(capability, quantity, ceiling_unit_price)
    catalog = Catalog(capability, listing.min_unit_price)
    store = store_dir / "erebus-mock-store.json"
    buyer = MockErebusClient(identity=BUYER, store_path=store, latency_seconds=latency,
                             spendable_notes=[requisition.ceiling])
    supplier = MockErebusClient(identity=listing.seller, store_path=store, latency_seconds=latency, spendable_notes=[])
    state = await run_negotiation(
        buyer_client=buyer, seller_client=supplier, buyer_address=BUYER, seller_address=listing.seller,
        buyer_policy=buyer_policy(BUYER, requisition, max_rounds=max_rounds),
        seller_policy=supplier_policy(listing.seller, catalog, requisition, max_rounds=max_rounds),
        token=listing.asset, max_rounds=max_rounds)
    if state.settlement is None:
        return {"matched": True, "settled": False, "listing": listing.to_dict(),
                "capability": capability, "ceiling": requisition.ceiling}

    accepted = next(offer for offer in state.offers if offer.offer_id == state.settlement.acceptance)
    deal_ref = hashlib.sha256(
        f"{listing.service_id}:{accepted.deal_id}:{state.settlement.agreed_amount}:"
        f"{state.settlement.paid_amount}".encode()).hexdigest()
    delivery = mark_delivered(
        DeliveryRecord(deal_ref, listing.service_id, DeliveryStatus.PENDING),
        hashlib.sha256(resource).hexdigest())
    review = review_disclosure(
        DisclosedRecord(channel_id=accepted.channel_id, participants=[BUYER, listing.seller],
                        offers=state.offers, settlement=state.settlement),
        requisition)
    return {
        "matched": True,
        "settled": True,
        "listing": listing.to_dict(),
        "capability": capability,
        "ceiling": requisition.ceiling,
        "agreed_amount": state.settlement.agreed_amount,
        "paid_amount": state.settlement.paid_amount,
        "deal_ref": deal_ref,
        "delivery": {"status": delivery.status.value, "artifact_sha256": delivery.artifact_sha256,
                     "is_settlement_evidence": False},
        "audit": {"compliant": review.compliant, "reasons": list(review.reasons)},
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capability", default="summarize-documents")
    parser.add_argument("--quantity", type=int, default=10)
    parser.add_argument("--ceiling-unit-price", type=int, default=8, help="the buyer's maximum unit price")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--latency", type=float, default=0.2)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stdout)
    registry = seed_registry()
    with tempfile.TemporaryDirectory() as tmp:
        record = asyncio.run(rehearse(
            args.capability, args.ceiling_unit_price, args.quantity, registry=registry,
            latency=args.latency, max_rounds=args.rounds, store_dir=Path(tmp)))
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
