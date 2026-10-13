"""Receipt explorer: distinct facts, no delivery claim, text and HTML rendering."""

import pytest

from erebus_agents.receipt_explorer import (
    DealReceipt,
    receipt_from_report,
    render_html,
    render_text,
)

COMMITMENT = "ab" * 32
ISSUER = "0xissuer"

AUDITOR_REPORT = {
    "operation_ref": "op-1",
    "auditor": {"agreement_verified": True, "payment_verified": True,
                "delivery_verified": False, "deal_commitment": COMMITMENT},
}


def test_receipt_rejects_bad_commitment_and_a_delivery_claim():
    with pytest.raises(ValueError):
        DealReceipt("xyz", ISSUER, True, True)
    with pytest.raises(ValueError, match="delivery"):
        DealReceipt(COMMITMENT, ISSUER, True, True, delivery_verified=True)
    with pytest.raises(ValueError, match="delivery"):
        receipt_from_report({"auditor": {**AUDITOR_REPORT["auditor"], "delivery_verified": True}}, issuer=ISSUER)


def test_status_keeps_the_facts_separate():
    assert DealReceipt(COMMITMENT, ISSUER, True, True).status == "verified"
    assert DealReceipt(COMMITMENT, ISSUER, True, False).status == "agreement_only"
    assert DealReceipt(COMMITMENT, ISSUER, False, False).status == "unverified"


def test_receipt_from_auditor_report_maps_the_independent_flags():
    receipt = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, parties=("0xa", "0xb"))
    assert receipt.agreement_verified and receipt.payment_verified and not receipt.delivery_verified
    assert receipt.operation_ref == "op-1" and receipt.parties == ("0xa", "0xb")


def test_only_an_auditor_report_is_accepted():
    with pytest.raises(ValueError, match="auditor"):
        receipt_from_report({"settled": True, "deal_ref": COMMITMENT}, issuer=ISSUER)


def test_an_agreement_only_audit_leaves_payment_pending():
    # metropolis_audit.py without a deployment verifies the agreement and reports no payment flag.
    report = {"operation_ref": "op-1", "auditor": {"agreement_verified": True, "payment_verified": None,
                                                    "delivery_verified": False, "deal_commitment": COMMITMENT}}
    receipt = receipt_from_report(report, issuer=ISSUER)
    assert receipt.status == "agreement_only" and receipt.payment_verified is False


def test_commitment_override_fills_a_gap_but_never_replaces_a_verified_one():
    missing = {"auditor": {**AUDITOR_REPORT["auditor"], "deal_commitment": None}}
    assert receipt_from_report(missing, issuer=ISSUER, deal_commitment=COMMITMENT).deal_commitment == COMMITMENT
    same = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, deal_commitment=COMMITMENT.upper())
    assert same.deal_commitment == COMMITMENT
    with pytest.raises(ValueError, match="auditor verified"):
        receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, deal_commitment="cd" * 32)


def test_text_render_flags_pending_without_calling_it_failure():
    text = render_text(receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, asset="0xusdc"))
    assert "Status:     verified" in text and "Delivery:   not proven" in text and "Asset:      0xusdc" in text
    pending = render_text(DealReceipt(COMMITMENT, ISSUER, True, False))
    assert "Payment:    pending" in pending and "not evidence of non-payment" in pending


def test_html_render_is_self_contained_and_escapes_values():
    rendered = render_html(receipt_from_report(AUDITOR_REPORT, issuer="0x<script>"))
    assert rendered.startswith("<!doctype html>") and "<style>" in rendered
    assert "&lt;script&gt;" in rendered and "0x<script>" not in rendered
    assert "not proven (disclosure never proves delivery)" in rendered


def test_to_dict_is_json_shaped():
    payload = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, parties=("0xa",)).to_dict()
    assert payload["deal_commitment"] == COMMITMENT and payload["parties"] == ["0xa"]
    assert payload["delivery_verified"] is False and payload["status"] == "verified"
