"""Paid-gateway rehearsal: one payment per call, retries reuse the permit, never a payment claim."""

import asyncio
import secrets
import time

import pytest

from erebus_agents.api_gateway_demo import rehearse
from erebus_agents.metropolis_loop import drive_x402_deal
from erebus_agents.paid_gateway import GatewayEndpoint, LocalGateway

ENDPOINT = GatewayEndpoint("svc-gateway", "summarize-document", 25, "0xusdc")


def test_endpoint_rejects_empty_identity_or_nonpositive_price():
    with pytest.raises(ValueError):
        GatewayEndpoint("", "c", 1, "0xa")
    with pytest.raises(ValueError):
        GatewayEndpoint("svc", "c", 0, "0xa")


async def _no_wait(_seconds: float) -> None:
    return None


def test_each_call_pays_exactly_once():
    started = time.monotonic()
    record = asyncio.run(rehearse(ENDPOINT, 3))
    assert time.monotonic() - started < 1.0, "the offline rehearsal must not sleep between retries"
    assert record["negotiate_calls"] == 6, "one negotiation per side per call, none on retry"
    assert record["permits_signed"] == 3 and record["payments_collected"] == 3
    assert record["total_billed"] == 75
    assert record["payment_verified"] is False
    assert all(call["resource_bytes"] == len(b"paid resource body") for call in record["calls"])


def test_driver_retries_only_by_retrieval_never_by_renegotiating():
    # The permit/payment counters are the model's own bookkeeping; what this pins about the real
    # driver is that every retry reaches the seller as a retrieval and never as a new negotiation.
    gateway = LocalGateway(ENDPOINT)
    record = asyncio.run(drive_x402_deal(gateway.buyer_session(), gateway.seller_session(), secrets.token_hex(32),
                                         sleep=_no_wait))
    assert gateway.negotiate_calls == 2 and gateway.retrieve_calls == 3
    assert record["retrieval_attempts"] == 3
    assert record["retrieval_states"] == ["payment_pending", "paid_but_undelivered", "retrieved"]
    assert gateway.permits_signed == 1 and gateway.payments_collected == 1
    assert record["payment_verified"] is False and record["payment_verification"] == "independent auditor only"


def test_immediate_serving_needs_no_retry():
    gateway = LocalGateway(ENDPOINT, pending_states=())
    record = asyncio.run(drive_x402_deal(gateway.buyer_session(), gateway.seller_session(), secrets.token_hex(32),
                                         sleep=_no_wait))
    assert record["retrieval_attempts"] == 1
    assert gateway.permits_signed == 1 and gateway.payments_collected == 1


def test_seller_session_does_not_retrieve_and_unknown_tools_are_refused():
    gateway = LocalGateway(ENDPOINT)
    with pytest.raises(ValueError, match="no tool"):
        asyncio.run(gateway.seller_session().call_tool("retrieve_service_access", {"evidence_name": "x.0.tx"}))


def test_rehearsal_rejects_a_nonpositive_call_count():
    with pytest.raises(ValueError):
        asyncio.run(rehearse(ENDPOINT, 0))
