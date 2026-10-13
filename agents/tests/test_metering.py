"""Metered access: read the real allowance, spend calls, retry pending by calling, never overdraw."""

import asyncio

import pytest

from erebus_agents.metering import Allowance, consume
from erebus_agents.metropolis_loop import HarnessError
from erebus_agents.saas_metering_demo import buy_and_consume, evidence_name, run

OPERATION = "34" * 32
COMMITMENT = "cd" * 32
ENTITLEMENT = "ee" * 32


def status(remaining, quantity=5):
    return {"ok": True, "result": {"status": "allowance", "result": {
        "deal_commitment": COMMITMENT, "entitlement_id": ENTITLEMENT, "quantity": quantity,
        "seller_next_call_index": quantity - remaining, "remaining": remaining,
        "next_call_index": quantity - remaining, "in_flight": None, "exhausted": remaining == 0,
        "recovered_calls": 0, "seller_reported_payment_finalized": True, "payment_verified": False,
        "delivery_verified": False}}}


def issued(index, remaining):
    return {"ok": True, "result": {"status": "issued", "result": {
        "deal_commitment": COMMITMENT, "entitlement_id": ENTITLEMENT, "call_index": index, "quantity": 5,
        "next_call_index": index + 1, "remaining": remaining, "response_sha256": "ab" * 32, "response_bytes": 9,
        "response_file": f"/cache/metered/calls/{'ab' * 32}.0.tx", "repeated": False, "cached": False, "late": False,
        "seller_reported_payment_finalized": True, "response_hash_consistent": True, "resource_verified": False,
        "payment_verified": False, "delivery_verified": False}}}


PENDING = {"ok": False, "result": {"status": "pending", "retry_without_payment": True, "payment_verified": False}}


class Session:
    def __init__(self, **replies):
        self.replies = {name: list(values) for name, values in replies.items()}
        self.calls = []

    async def call_tool(self, name, arguments):
        if name in {"metered_service_status", "call_metered_service"}:
            assert arguments == {"evidence_name": evidence_name(OPERATION)}
        self.calls.append(name)
        return self.replies[name].pop(0)


async def no_sleep(_):
    pass


def test_spends_the_allowance_and_refuses_to_overdraw():
    buyer = Session(metered_service_status=[status(2), status(0)],
                    call_metered_service=[issued(3, 1), issued(4, 0)])
    usage = asyncio.run(consume(buyer, evidence_name(OPERATION), 4, sleep=no_sleep))
    assert [call["call_index"] for call in usage["calls_issued"]] == [3, 4]
    assert buyer.calls.count("call_metered_service") == 2, "no call is attempted past exhaustion"
    assert "exhausted" in usage["refused"] and usage["after"]["exhausted"] is True
    assert usage["payment_verified"] is False


def test_a_pending_call_is_retried_by_calling_again_never_by_paying():
    buyer = Session(metered_service_status=[status(1), status(0)], call_metered_service=[PENDING, PENDING, issued(4, 0)])
    usage = asyncio.run(consume(buyer, evidence_name(OPERATION), 1, sleep=no_sleep))
    assert len(usage["calls_issued"]) == 1 and usage["refused"] is None
    assert buyer.calls.count("call_metered_service") == 3


def test_a_non_retryable_failure_stops_and_a_verification_claim_is_refused():
    rejected = {"ok": False, "result": {"status": "error", "retry_without_payment": False}}
    with pytest.raises(HarnessError, match="non-retryable"):
        asyncio.run(consume(Session(metered_service_status=[status(1)], call_metered_service=[rejected]),
                            evidence_name(OPERATION), 1, sleep=no_sleep))
    lying = issued(4, 0)
    lying["result"]["result"]["payment_verified"] = True
    with pytest.raises(HarnessError, match="must not claim"):
        asyncio.run(consume(Session(metered_service_status=[status(1)], call_metered_service=[lying]),
                            evidence_name(OPERATION), 1, sleep=no_sleep))


def test_allowance_parses_the_seller_report():
    allowance = Allowance.from_result(status(2)["result"]["result"])
    assert allowance.quantity == 5 and allowance.remaining == 2 and not allowance.exhausted


NEGOTIATED = {"ok": True, "result": {"deal_commitment": COMMITMENT, "measurements_ms": {"negotiation": 5},
                                     "evidence_file": f"/operator/state/agent/{OPERATION}.0.tx"}}
FINALIZED = {"ok": True, "result": {"status": "ok", "payment_verified": True, "stage": "Finalized",
                                    "winning_commitment": COMMITMENT, "measurements_ms": {}}}


def test_one_settlement_buys_the_bundle_then_calls_spend_it():
    buyer = Session(negotiate_deal=[NEGOTIATED], check_settlement_funding=[{"ok": True, "result": {"status": "ready"}}],
                    settle_deal=[FINALIZED], metered_service_status=[status(5), status(4)],
                    call_metered_service=[issued(0, 4)])
    record = asyncio.run(buy_and_consume(buyer, Session(negotiate_deal=[NEGOTIATED]), OPERATION, 1, sleep=no_sleep))
    assert record["settlement_payment_verified"] is True and record["settle_calls"] == 1
    assert record["usage"]["after"]["remaining"] == 4


def test_consume_only_never_touches_the_settlement_tools():
    buyer = Session(metered_service_status=[status(4), status(3)], call_metered_service=[issued(1, 3)])
    record = asyncio.run(buy_and_consume(buyer, Session(), OPERATION, 1, consume_only=True, sleep=no_sleep))
    assert "settle_deal" not in buyer.calls and "negotiate_deal" not in buyer.calls
    assert record["usage"]["after"]["remaining"] == 3


def test_the_buyer_operator_must_select_the_metered_profile():
    with pytest.raises(ValueError, match="metered"):
        asyncio.run(run("/b.json", "/p.json", {"EREBUS_ACCESS_PROFILE": "snapshot"}, "/s.json", OPERATION, 1))
