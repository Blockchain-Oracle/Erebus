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
REHEARSAL_REPORT = {"settled": True, "agreed_amount": 64, "paid_amount": 64, "deal_ref": COMMITMENT,
                    "audit": {"compliant": True, "reasons": []}}


def test_receipt_rejects_bad_commitment_and_a_delivery_claim():
    with pytest.raises(ValueError):
        DealReceipt("xyz", ISSUER, True, True)
    with pytest.raises(ValueError, match="delivery"):
        DealReceipt(COMMITMENT, ISSUER, True, True, delivery_verified=True)


def test_consistency_and_status_keep_the_three_facts_separate():
    assert DealReceipt(COMMITMENT, ISSUER, True, True).status == "verified"
    assert DealReceipt(COMMITMENT, ISSUER, True, False).status == "agreement_only"
    assert DealReceipt(COMMITMENT, ISSUER, False, False).status == "unverified"
    assert DealReceipt(COMMITMENT, ISSUER, True, True, agreed_amount=10, paid_amount=10).consistency == "consistent"
    assert DealReceipt(COMMITMENT, ISSUER, True, True, agreed_amount=10, paid_amount=9).consistency == "inconsistent"
    assert DealReceipt(COMMITMENT, ISSUER, True, False).consistency == "unknown"


def test_receipt_from_auditor_report_maps_the_independent_flags():
    receipt = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, parties=("0xa", "0xb"))
    assert receipt.agreement_verified and receipt.payment_verified and not receipt.delivery_verified
    assert receipt.operation_ref == "op-1" and receipt.source == "disclosure"


def test_receipt_from_rehearsal_report_is_not_independent_verification():
    receipt = receipt_from_report(REHEARSAL_REPORT, issuer=ISSUER)
    assert not receipt.agreement_verified and not receipt.payment_verified
    assert receipt.status == "rehearsal" and receipt.reported_settled is True
    assert receipt.agreed_amount == receipt.paid_amount == 64 and receipt.source == "rehearsal"
    text = render_text(receipt)
    assert "Status:     rehearsal" in text
    assert not any(line.rstrip().endswith(": verified") or line.split(":", 1)[-1].strip() == "verified"
                   for line in text.splitlines())
    assert "Status</th><td class=\"warn\">rehearsal" in render_html(receipt)


def test_a_rehearsal_receipt_cannot_claim_verification():
    with pytest.raises(ValueError, match="rehearsal"):
        DealReceipt(COMMITMENT, ISSUER, True, True, source="rehearsal")
    with pytest.raises(ValueError, match="source"):
        DealReceipt(COMMITMENT, ISSUER, True, True, source="self-reported")


def test_commitment_override_fills_a_gap_but_never_replaces_a_verified_one():
    missing = {"auditor": {**AUDITOR_REPORT["auditor"], "deal_commitment": None}}
    assert receipt_from_report(missing, issuer=ISSUER, deal_commitment=COMMITMENT).deal_commitment == COMMITMENT
    same = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, deal_commitment=COMMITMENT.upper())
    assert same.deal_commitment == COMMITMENT
    with pytest.raises(ValueError, match="auditor verified"):
        receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, deal_commitment="cd" * 32)
    rehearsal = {k: v for k, v in REHEARSAL_REPORT.items() if k != "deal_ref"}
    assert receipt_from_report(rehearsal, issuer=ISSUER, deal_commitment=COMMITMENT).deal_commitment == COMMITMENT


def test_text_render_flags_pending_without_calling_it_failure():
    text = render_text(receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, asset="0xusdc"))
    assert "Status:     verified" in text
    assert "Delivery:   not proven" in text
    pending = render_text(DealReceipt(COMMITMENT, ISSUER, True, False))
    assert "Payment:    pending" in pending
    assert "not evidence of non-payment" in pending


def test_html_render_is_self_contained_and_escapes_values():
    rendered = render_html(receipt_from_report(AUDITOR_REPORT, issuer="0x<script>"))
    assert rendered.startswith("<!doctype html>") and "<style>" in rendered
    assert "&lt;script&gt;" in rendered and "0x<script>" not in rendered
    assert "not proven (disclosure never proves delivery)" in rendered


def test_to_dict_is_json_shaped():
    payload = receipt_from_report(AUDITOR_REPORT, issuer=ISSUER, parties=("0xa",)).to_dict()
    assert payload["deal_commitment"] == COMMITMENT and payload["parties"] == ["0xa"]
    assert payload["delivery_verified"] is False and payload["status"] == "verified"
