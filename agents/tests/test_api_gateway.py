"""Pay-per-call over x402-exact: one deal per call, retries only by retrieval, resumable ledger."""

import asyncio
import json

import pytest

from erebus_agents.api_gateway_demo import load_or_create_ledger, pay_per_call, run_calls

COMMITMENT = "cd" * 32


class Gateway:
    """Scripted buyer/seller replies in the shape the real x402 MCP tools return."""

    def __init__(self, pending=("payment_pending",), fail_on=None):
        self.pending = list(pending)
        self.fail_on = fail_on
        self.calls = []
        self.attempts = {}

    def session(self, side):
        gateway = self

        class Session:
            async def call_tool(self, name, arguments):
                gateway.calls.append((side, name))
                if name == "negotiate_deal":
                    ref = arguments["operation_ref"]
                    return {"ok": True, "result": {"deal_commitment": COMMITMENT,
                                                   "evidence_file": f"/operator/state/agent/{ref}.0.tx"}}
                assert side == "buyer" and name == "retrieve_service_access"
                ref = arguments["evidence_name"].split(".")[0]
                if ref == gateway.fail_on:
                    return {"ok": False, "result": {"status": "rejected", "retry_without_payment": False}}
                attempt = gateway.attempts.get(ref, 0)
                gateway.attempts[ref] = attempt + 1
                if attempt < len(gateway.pending):
                    return {"ok": False, "result": {"status": gateway.pending[attempt], "retry_without_payment": True}}
                return {"ok": True, "result": {"status": "retrieved", "result": {
                    "resource_verified": True, "payment_verified": False, "delivery_verified": False,
                    "resource_sha256": "ef" * 32, "resource_bytes": 18, "seller_reported_payment_finalized": True}}}

        return Session()


async def no_sleep(_):
    pass


REFS = ["01" * 32, "02" * 32, "03" * 32]


def test_each_call_is_one_deal_and_retries_never_renegotiate():
    gateway = Gateway(pending=("payment_pending", "paid_but_undelivered"))
    record = asyncio.run(run_calls(gateway.session("buyer"), gateway.session("seller"), REFS, sleep=no_sleep))
    assert record["completed"] == 3 and record["failed"] is None and record["remaining"] == []
    assert sum(1 for _, name in gateway.calls if name == "negotiate_deal") == 6, "one per side per call"
    assert all(call["retrieval_attempts"] == 3 for call in record["calls"])
    assert record["payment_verified"] is False and record["payment_verification"] == "independent auditor only"
    assert "public" in record["rails_note"]


def test_a_failed_call_stops_the_run_and_reports_what_to_resume():
    gateway = Gateway(pending=(), fail_on=REFS[1])
    record = asyncio.run(run_calls(gateway.session("buyer"), gateway.session("seller"), REFS, sleep=no_sleep))
    assert record["completed"] == 1 and record["failed"]["operation_ref"] == REFS[1]
    assert record["remaining"] == REFS[1:]


def test_the_ledger_is_created_once_owner_only_and_reused_on_resume(tmp_path):
    path = tmp_path / "calls.json"
    refs = load_or_create_ledger(path, 3)
    assert len(set(refs)) == 3 and all(len(ref) == 64 for ref in refs)
    assert (path.stat().st_mode & 0o777) == 0o600
    assert load_or_create_ledger(path, 3) == refs, "a resumed run never gets fresh IDs"
    with pytest.raises(ValueError, match="resume"):
        load_or_create_ledger(path, 4)
    with pytest.raises(ValueError):
        load_or_create_ledger(tmp_path / "other.json", 0)
    assert json.loads(path.read_text()) == {"operation_refs": refs}


def test_the_buyer_operator_must_select_the_x402_rail():
    with pytest.raises(ValueError, match="x402-exact"):
        asyncio.run(pay_per_call("/b.json", {"EREBUS_ACCESS_PAYMENT_RAIL": "observe"}, "/s.json", REFS))
