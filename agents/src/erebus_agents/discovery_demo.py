"""Rehearsal of the discovery directory: publish, search by capability/asset/price, resolve.

Builds a small index of service records, prints the cheapest matches for one capability, and
resolves the winning seller's endpoint. Records may carry the digest of the seller's signed
descriptor; this rehearsal does not verify it -- verification is Rust's `ServiceDescriptor`.
"""

from __future__ import annotations

import json
import sys
from typing import Any

from erebus_agents.discovery import DiscoveryDirectory, ServiceRecord

CAPABILITY = "summarize-documents"
DIGEST = "ab" * 32


def seed() -> DiscoveryDirectory:
    directory = DiscoveryDirectory()
    directory.publish(ServiceRecord("svc-atlas", "0xatlas", CAPABILITY, "0xusdc", 3, 7,
                                    "tcp://atlas.example:7443", descriptor_digest=DIGEST))
    directory.publish(ServiceRecord("svc-aegean", "0xaegean", CAPABILITY, "0xusdc", 4, 9,
                                    "tcp://aegean.example:7443"))
    directory.publish(ServiceRecord("svc-boreas", "0xboreas", CAPABILITY, "0xwmon", 6, 12,
                                    "tcp://boreas.example:7443", descriptor_digest="cd" * 32))
    return directory


def rehearse() -> dict[str, Any]:
    directory = seed()
    usdc = directory.search(CAPABILITY, asset="0xusdc", max_unit_price=8)
    signed = directory.search(CAPABILITY, require_descriptor=True)
    winner = usdc[0].service_id if usdc else None
    return {
        "capability": CAPABILITY,
        "matches": [{"service_id": record.service_id, "seller": record.seller, "asset": record.asset,
                     "min_unit_price": record.min_unit_price, "endpoint": record.endpoint,
                     "descriptor_digest": record.descriptor_digest} for record in directory.records()],
        "usdc_under_8": [record.service_id for record in usdc],
        "with_signed_descriptor": [record.service_id for record in signed],
        "resolved_endpoint": directory.resolve(winner) if winner else None,
    }


def main() -> None:
    json.dump(rehearse(), sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
