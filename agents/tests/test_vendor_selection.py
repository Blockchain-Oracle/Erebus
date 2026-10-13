"""Vendor selection: outcome validation, selection, and settlement-free negotiation."""

import asyncio

import pytest

from erebus_agents.procurement import Catalog, Requisition, buyer_policy, supplier_policy
from erebus_agents.vendor_selection import VendorOutcome, negotiate_amount, select
from erebus_agents.vendor_selection_demo import CAPABILITY, rehearse

AT = ("svc-atlas", "0xatlas", True, 64)
AE = ("svc-aegean", "0xaegean", True, 70)
BOREAS = ("svc-boreas", "0xboreas", False, None)


def test_outcome_validation():
    with pytest.raises(ValueError):
        VendorOutcome("", "0xa", True, 1)
    with pytest.raises(ValueError):
        VendorOutcome("svc", "0xa", True, None)
    with pytest.raises(ValueError):
        VendorOutcome("svc", "0xa", False, 5)


def test_select_picks_cheapest_valid_and_explains_the_rest():
    outcomes = (VendorOutcome(*AT), VendorOutcome(*AE), VendorOutcome(*BOREAS),
                VendorOutcome("svc-delta", "0xdelta", True, 90))
    selection = select(outcomes, ceiling=80)
    assert selection.winner is not None and selection.winner.service_id == "svc-atlas"
    reasons = dict(selection.rejected)
    assert reasons == {"svc-aegean": "outbid", "svc-boreas": "no agreement",
                       "svc-delta": "above the buyer's ceiling"}


def test_select_reports_no_winner_when_nothing_qualifies():
    selection = select((VendorOutcome(*BOREAS), VendorOutcome("svc-delta", "0xdelta", True, 90)), ceiling=80)
    assert not selection.awarded and selection.winner is None


def test_negotiate_amount_agrees_without_settling(tmp_path):
    from erebus_mcp.mock_client import MockErebusClient

    requisition = Requisition(CAPABILITY, 10, 8)
    store = tmp_path / "erebus-mock-store.json"
    buyer = MockErebusClient(identity="0xbuyer", store_path=store, latency_seconds=0.0,
                             spendable_notes=[requisition.ceiling])
    seller = MockErebusClient(identity="0xatlas", store_path=store, latency_seconds=0.0, spendable_notes=[])
    amount = asyncio.run(negotiate_amount(
        buyer_client=buyer, seller_client=seller, buyer_address="0xbuyer", seller_address="0xatlas",
        token="0xusdc", buyer_policy=buyer_policy("0xbuyer", requisition),
        seller_policy=supplier_policy("0xatlas", Catalog(CAPABILITY, 3), requisition)))
    assert amount == 64
    handle = asyncio.run(buyer.open_channel("0xatlas"))
    assert asyncio.run(buyer.read_channel_state(handle)).settlement is None


def test_rehearsal_negotiates_with_all_and_pays_one(tmp_path):
    record = asyncio.run(rehearse(CAPABILITY, 10, 8, store_dir=tmp_path))
    assert [item["agreed_amount"] for item in record["negotiated"]] == [64, 70, None]
    assert record["selection"]["winner"] == "svc-atlas" and record["settled"]
    assert record["paid_amount"] == 64 and record["audit"]["compliant"]
