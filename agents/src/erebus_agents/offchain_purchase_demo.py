"""Rehearsal of one-off off-chain purchases: compute, a dataset, or an API response.

These are the same shape as the marketplace loop -- one buyer, one seller, one private
settlement, an off-chain deliverable -- so this reuses `marketplace.rehearse` and only swaps the
capability and the candidate listings. It is here to show that "buy compute" and "buy a dataset"
need no new mechanism, not to add one.
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

from erebus_agents.marketplace import ServiceListing, ServiceRegistry
from erebus_agents.marketplace_demo import ASSET, rehearse

KINDS: dict[str, tuple[str, tuple[ServiceListing, ...]]] = {
    "compute": ("run-model", (
        ServiceListing("svc-atlas", "0xatlas", "run-model", ASSET, "gpu-hour", 3, 7),
        ServiceListing("svc-aegean", "0xaegean", "run-model", ASSET, "gpu-hour", 5, 9),
    )),
    "dataset": ("purchase-dataset", (
        ServiceListing("svc-atlas", "0xatlas", "purchase-dataset", ASSET, "record", 2, 6),
        ServiceListing("svc-boreas", "0xboreas", "purchase-dataset", ASSET, "record", 4, 8),
    )),
    "api": ("call-api", (
        ServiceListing("svc-aegean", "0xaegean", "call-api", ASSET, "call", 1, 4),
    )),
}


async def rehearse_purchase(kind: str, ceiling_unit_price: int, quantity: int, *, latency: float = 0.0,
                            max_rounds: int = 3, store_dir: Path) -> dict[str, Any]:
    if kind not in KINDS:
        raise ValueError(f"kind must be one of {sorted(KINDS)}")
    capability, listings = KINDS[kind]
    registry = ServiceRegistry()
    for listing in listings:
        registry.publish(listing)
    record = await rehearse(capability, ceiling_unit_price, quantity, registry=registry,
                            latency=latency, max_rounds=max_rounds, store_dir=store_dir)
    return {"kind": kind, **record}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kind", choices=sorted(KINDS), default="compute")
    parser.add_argument("--quantity", type=int, default=10)
    parser.add_argument("--ceiling-unit-price", type=int, default=8)
    parser.add_argument("--latency", type=float, default=0.2)
    args = parser.parse_args()
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stdout)
    with tempfile.TemporaryDirectory() as tmp:
        record = asyncio.run(rehearse_purchase(args.kind, args.ceiling_unit_price, args.quantity,
                                               latency=args.latency, store_dir=Path(tmp)))
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
