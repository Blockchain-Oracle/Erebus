"""Off-chain purchases are marketplace purchases under a kind-specific capability."""

import pytest

from erebus_agents.marketplace import PurchaseRoute, ServiceListing, ServiceRegistry
from erebus_agents.marketplace_demo import select_listing
from erebus_agents.offchain_purchase_demo import KINDS, capability_for


def test_every_kind_maps_to_a_capability_and_unknown_kinds_are_refused():
    assert {kind: capability_for(kind) for kind in KINDS} == KINDS
    with pytest.raises(ValueError):
        capability_for("teleport")


def test_a_kind_selects_through_the_same_marketplace_routes():
    registry = ServiceRegistry()
    registry.publish(ServiceListing("svc-gpu", "0xatlas", capability_for("compute"), "0xusdc", "gpu-hour", 3, 7))
    registry.publish(ServiceListing("svc-data", "0xboreas", capability_for("dataset"), "0xusdc", "record", 2, 6))
    routes = {"svc-gpu": PurchaseRoute("svc-gpu", "/b.json", "/p.json", "/s.json")}
    listing, route = select_listing(registry, routes, capability_for("compute"), 8)
    assert listing.service_id == route.service_id == "svc-gpu"
    assert select_listing(registry, routes, capability_for("dataset"), 8) is None, "no route, no purchase"
