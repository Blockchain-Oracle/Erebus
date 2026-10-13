"""Marketplace registry, routes and delivery-state policy, and the purchase over the real loop."""

import asyncio
import json

import pytest

from erebus_agents.marketplace import (
    DeliveryRecord,
    DeliveryStatus,
    PurchaseRoute,
    ServiceListing,
    ServiceRegistry,
    load_routes,
    mark_delivered,
    mark_disputed,
    mark_refunded,
)
from erebus_agents.marketplace_demo import purchase_record, select_listing
from erebus_agents.metropolis_loop import drive_deal

DIGEST = "ab" * 32
OPERATION = "12" * 32
COMMITMENT = "cd" * 32
LISTINGS = (
    ServiceListing("svc-atlas", "0xatlas", "summarize-documents", "0xusdc", "page", 3, 7),
    ServiceListing("svc-aegean", "0xaegean", "summarize-documents", "0xusdc", "page", 4, 9),
    ServiceListing("svc-boreas", "0xboreas", "summarize-documents", "0xusdc", "page", 6, 12),
)


def seed_registry():
    registry = ServiceRegistry()
    for entry in LISTINGS:
        registry.publish(entry)
    return registry


def route(service_id, access=None):
    return PurchaseRoute(service_id, f"/operator/{service_id}/buyer.negotiation.json",
                         "/operator/buyer.payment.json", f"/operator/{service_id}/seller.negotiation.json", access)


def listing(service_id="svc-a", seller="0xseller", capability="summarize", minimum=3, maximum=7):
    return ServiceListing(service_id, seller, capability, "0xusdc", "page", minimum, maximum)


def test_listing_rejects_bad_identifiers_and_price_bands():
    with pytest.raises(ValueError):
        ServiceListing("Svc A", "0xs", "c", "0xu", "page", 1, 2)
    with pytest.raises(ValueError):
        listing(minimum=5, maximum=4)
    with pytest.raises(ValueError):
        listing(minimum=0)


def test_registry_publish_is_idempotent_but_a_conflict_is_refused(tmp_path):
    registry = ServiceRegistry()
    registry.publish(listing())
    registry.publish(listing())
    with pytest.raises(ValueError, match="already published"):
        registry.publish(listing(seller="0xother"))


def test_registry_search_is_cheapest_first_and_caps_at_the_ceiling():
    registry = seed_registry()
    assert [entry.service_id for entry in registry.search("summarize-documents")] == [
        "svc-atlas", "svc-aegean", "svc-boreas"]
    capped = registry.search("summarize-documents", max_unit_price=4)
    assert [entry.service_id for entry in capped] == ["svc-atlas", "svc-aegean"]
    assert registry.best("summarize-documents", max_unit_price=4).service_id == "svc-atlas"
    assert registry.best("summarize-documents", max_unit_price=2) is None


def test_registry_retire_and_round_trip(tmp_path):
    registry = seed_registry()
    path = registry.save(tmp_path / "registry.json")
    assert (path.stat().st_mode & 0o777) == 0o600
    loaded = ServiceRegistry.load(path)
    assert [entry.to_dict() for entry in loaded.listings()] == [entry.to_dict() for entry in registry.listings()]
    loaded.retire("svc-atlas")
    assert "svc-atlas" not in [entry.service_id for entry in loaded.listings()]
    with pytest.raises(KeyError):
        loaded.retire("svc-atlas")


def test_delivery_transitions_are_one_way_and_need_an_artifact():
    record = DeliveryRecord("deal", "svc-a", DeliveryStatus.PENDING)
    with pytest.raises(ValueError):
        mark_delivered(record, "not-hex")
    delivered = mark_delivered(record, DIGEST.upper())
    assert delivered.status is DeliveryStatus.DELIVERED and delivered.artifact_sha256 == DIGEST
    with pytest.raises(ValueError):
        mark_refunded(delivered)
    disputed = mark_disputed(delivered)
    assert disputed.status is DeliveryStatus.DISPUTED and disputed.artifact_sha256 == DIGEST
    assert mark_refunded(disputed).status is DeliveryStatus.REFUNDED


def test_delivery_is_never_settlement_evidence():
    delivered = mark_delivered(DeliveryRecord("deal", "svc-a", DeliveryStatus.PENDING), DIGEST)
    assert not hasattr(delivered, "payment_verified")


def test_routes_need_absolute_operator_files_and_load_once_per_listing(tmp_path):
    with pytest.raises(ValueError, match="absolute"):
        PurchaseRoute("svc-a", "buyer.json", "/p.json", "/s.json")
    with pytest.raises(ValueError, match="absolute"):
        PurchaseRoute("svc-a", "/b.json", "/p.json", "/s.json", "access.json")
    path = tmp_path / "routes.json"
    entry = {"service_id": "svc-atlas", "buyer_negotiation_config": "/b.json",
             "buyer_payment_config": "/p.json", "seller_negotiation_config": "/s.json"}
    path.write_text(json.dumps({"routes": [entry]}))
    assert load_routes(path)["svc-atlas"].buyer_access_file is None
    path.write_text(json.dumps({"routes": [entry, entry]}))
    with pytest.raises(ValueError, match="routed twice"):
        load_routes(path)


def test_selection_takes_the_cheapest_listing_this_buyer_can_actually_reach():
    registry = seed_registry()
    routes = {name: route(name) for name in ("svc-atlas", "svc-aegean")}
    listing, chosen = select_listing(registry, routes, "summarize-documents", 8)
    assert listing.service_id == chosen.service_id == "svc-atlas"
    # An unrouted listing is skipped, never bought through some default server.
    listing, _ = select_listing(registry, {"svc-aegean": route("svc-aegean")}, "summarize-documents", 8)
    assert listing.service_id == "svc-aegean"
    assert select_listing(registry, routes, "summarize-documents", 2) is None
    assert select_listing(registry, {}, "summarize-documents", 8) is None


class Session:
    """Scripted replies in the shape the real Metropolis MCP tools return."""

    def __init__(self, **replies):
        self.replies = {name: list(values) for name, values in replies.items()}
        self.calls = []

    async def call_tool(self, name, arguments):
        self.calls.append(name)
        return self.replies[name].pop(0)


async def no_sleep(_):
    pass


NEGOTIATED = {"ok": True, "result": {"deal_commitment": COMMITMENT, "measurements_ms": {"negotiation": 5},
                                     "evidence_file": f"/operator/state/agent/{OPERATION}.0.tx"}}
FINALIZED = {"ok": True, "result": {"status": "ok", "payment_verified": True, "stage": "Finalized",
                                    "winning_commitment": COMMITMENT, "measurements_ms": {"submission": 3},
                                    "chain_times": {"inclusion_block": 7}}}
RETRIEVED = {"ok": True, "result": {"status": "retrieved", "result": {
    "resource_verified": True, "resource_sha256": DIGEST, "resource_bytes": 37}}}


def drive(deliver):
    buyer = Session(negotiate_deal=[NEGOTIATED], check_settlement_funding=[{"ok": True, "result": {"status": "ready"}}],
                    settle_deal=[FINALIZED], retrieve_service_access=[RETRIEVED])
    record = asyncio.run(drive_deal(buyer, Session(negotiate_deal=[NEGOTIATED]), OPERATION, deliver=deliver,
                                    sleep=no_sleep))
    return buyer, record


def test_a_finalized_purchase_with_access_records_the_verified_delivery():
    buyer, deal = drive(deliver=True)
    record = purchase_record(LISTINGS[0], deal)
    assert buyer.calls.count("settle_deal") == 1
    assert record["payment_verified"] is True and record["deal_commitment"] == COMMITMENT
    assert record["operation_ref"] == OPERATION and record["listing"]["service_id"] == "svc-atlas"
    assert record["delivery"] == {"status": "delivered", "artifact_sha256": DIGEST, "is_settlement_evidence": False}


def test_without_access_the_delivery_stays_pending_for_the_operator():
    _, deal = drive(deliver=False)
    record = purchase_record(LISTINGS[0], deal)
    assert record["payment_verified"] is True
    assert record["delivery"] == {"status": "pending", "artifact_sha256": None, "is_settlement_evidence": False}
