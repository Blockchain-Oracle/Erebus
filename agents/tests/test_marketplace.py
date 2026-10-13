"""Marketplace registry, discovery and delivery-state policy, plus the mock rehearsal."""

import asyncio

import pytest

from erebus_agents.marketplace import (
    DeliveryRecord,
    DeliveryStatus,
    ServiceListing,
    ServiceRegistry,
    mark_delivered,
    mark_disputed,
    mark_refunded,
)
from erebus_agents.marketplace_demo import rehearse, seed_registry

DIGEST = "ab" * 32


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


def run(capability, ceiling, tmp_path):
    return asyncio.run(rehearse(capability, ceiling, 10, registry=seed_registry(), store_dir=tmp_path))


def test_rehearsal_picks_the_cheapest_listing_and_audits_compliant(tmp_path):
    result = run("summarize-documents", 8, tmp_path)
    assert result["matched"] and result["settled"]
    assert result["listing"]["service_id"] == "svc-atlas"
    assert result["paid_amount"] == result["agreed_amount"] <= result["ceiling"]
    assert result["audit"]["compliant"]
    assert result["delivery"]["status"] == "delivered" and not result["delivery"]["is_settlement_evidence"]


def test_rehearsal_reports_no_match_when_the_ceiling_is_below_every_listing(tmp_path):
    result = run("summarize-documents", 2, tmp_path)
    assert result == {"matched": False, "capability": "summarize-documents", "ceiling_unit_price": 2}
