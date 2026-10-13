"""Mock rehearsal of the confidential RFQ desk: issue a request, collect sealed quotes, award one.

The buyer publishes a request with a hidden ceiling, suppliers quote, the desk awards the
cheapest valid quote, and the buyer negotiates and settles with the winner on the mock client.
Only the winner's identity reaches the settlement path; losing quotes stay in the buyer's local
record.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from erebus_mcp.interface import DisclosedRecord
from erebus_mcp.mock_client import MockErebusClient

from erebus_agents.agent import run_negotiation
from erebus_agents.procurement import Catalog, Requisition, buyer_policy, review_disclosure, supplier_policy
from erebus_agents.rfq import AwardStatus, Quote, RequestForQuote, award

BUYER = "0xbuyer"
ASSET = "0xusdc"


def default_quotes(rfq: RequestForQuote, at: int) -> tuple[Quote, ...]:
    return (
        Quote("0xatlas", rfq.rfq_id, unit_price=9, valid_until=at + 3600),
        Quote("0xaegean", rfq.rfq_id, unit_price=10, valid_until=at + 3600),
        Quote("0xboreas", rfq.rfq_id, unit_price=13, valid_until=at + 3600),
        Quote("0xlate", rfq.rfq_id, unit_price=4, valid_until=at - 1),
    )


async def rehearse(rfq: RequestForQuote, quotes: tuple[Quote, ...], *, at: int, latency: float = 0.0,
                   max_rounds: int = 3, store_dir: Path) -> dict[str, Any]:
    result = award(rfq, quotes, at=at)
    record: dict[str, Any] = {
        "rfq_id": rfq.rfq_id,
        "item": rfq.item,
        "quantity": rfq.quantity,
        "ceiling": rfq.ceiling,
        "quotes": len(quotes),
        "award": {"status": result.status.value,
                  "winner": result.winner.supplier if result.winner else None,
                  "unit_price": result.winner.unit_price if result.winner else None,
                  "rejected": [{"supplier": supplier, "reason": reason}
                               for supplier, reason in result.rejected]},
    }
    if result.status is not AwardStatus.AWARDED or result.winner is None:
        record["settled"] = False
        return record

    requisition = Requisition(rfq.item, rfq.quantity, rfq.max_unit_price)
    store = store_dir / "erebus-mock-store.json"
    buyer = MockErebusClient(identity=BUYER, store_path=store, latency_seconds=latency,
                             spendable_notes=[requisition.ceiling])
    winner = MockErebusClient(identity=result.winner.supplier, store_path=store,
                              latency_seconds=latency, spendable_notes=[])
    state = await run_negotiation(
        buyer_client=buyer, seller_client=winner, buyer_address=BUYER, seller_address=result.winner.supplier,
        buyer_policy=buyer_policy(BUYER, requisition, max_rounds=max_rounds),
        seller_policy=supplier_policy(result.winner.supplier, Catalog(rfq.item, result.winner.unit_price), requisition,
                                      max_rounds=max_rounds),
        token=ASSET, max_rounds=max_rounds)
    record["settled"] = state.settlement is not None
    if state.settlement is not None:
        accepted = next(offer for offer in state.offers if offer.offer_id == state.settlement.acceptance)
        review = review_disclosure(
            DisclosedRecord(channel_id=accepted.channel_id, participants=[BUYER, result.winner.supplier],
                            offers=state.offers, settlement=state.settlement),
            requisition)
        record["agreed_amount"] = state.settlement.agreed_amount
        record["paid_amount"] = state.settlement.paid_amount
        record["audit"] = {"compliant": review.compliant, "reasons": list(review.reasons)}
    return record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--item", default="gpu-hours")
    parser.add_argument("--quantity", type=int, default=100)
    parser.add_argument("--max-unit-price", type=int, default=12)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--latency", type=float, default=0.2)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stdout)
    at = int(time.time())
    rfq = RequestForQuote("rfq-1", args.item, args.quantity, args.max_unit_price, deadline=at + 3600)
    with tempfile.TemporaryDirectory() as tmp:
        record = asyncio.run(rehearse(rfq, default_quotes(rfq, at), at=at, latency=args.latency,
                                      max_rounds=args.rounds, store_dir=Path(tmp)))
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
