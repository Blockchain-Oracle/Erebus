"""Render one disclosed deal as a private receipt, for a terminal or a static HTML file.

The explorer does not verify anything by itself. It formats a verification result that an
auditor already produced -- from `metropolis_audit.py` (agreement and payment against chain
data) or from a rehearsal (`procurement_demo.py`, `marketplace_demo.py`). Its whole job is to
keep three facts distinct and never collapse them:

* the agreement matches the disclosed deal,
* the payment matched the agreement,
* delivery was observed off-chain (never proven by the protocol).

A receipt that claims delivery verification is rejected on construction. "Payment pending" is
rendered as pending, not as failure: on a public RPC it can stay pending indefinitely, and
pending is not evidence of non-payment.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

_COMMITMENT = re.compile(r"[0-9a-fA-F]{64}")


@dataclass(frozen=True)
class DealReceipt:
    deal_commitment: str
    issuer: str
    agreement_verified: bool
    payment_verified: bool
    delivery_verified: bool = False
    parties: tuple[str, ...] = ()
    agreed_amount: int | None = None
    paid_amount: int | None = None
    asset: str | None = None
    network: str | None = None
    operation_ref: str | None = None
    source: str = "disclosure"

    def __post_init__(self) -> None:
        if not _COMMITMENT.fullmatch(self.deal_commitment):
            raise ValueError("deal_commitment must be 64 hex digits")
        if not self.issuer:
            raise ValueError("issuer must not be empty")
        if self.delivery_verified:
            raise ValueError("disclosure never proves delivery; a receipt claiming it is rejected")
        object.__setattr__(self, "deal_commitment", self.deal_commitment.lower())

    @property
    def consistency(self) -> str:
        if self.paid_amount is None or self.agreed_amount is None:
            return "unknown"
        return "consistent" if self.paid_amount == self.agreed_amount else "inconsistent"

    @property
    def status(self) -> str:
        if self.agreement_verified and self.payment_verified:
            return "verified"
        if self.agreement_verified:
            return "agreement_only"
        return "unverified"

    def to_dict(self) -> dict[str, Any]:
        return {
            "deal_commitment": self.deal_commitment,
            "issuer": self.issuer,
            "parties": list(self.parties),
            "agreement_verified": self.agreement_verified,
            "payment_verified": self.payment_verified,
            "delivery_verified": self.delivery_verified,
            "agreed_amount": self.agreed_amount,
            "paid_amount": self.paid_amount,
            "consistency": self.consistency,
            "asset": self.asset,
            "network": self.network,
            "operation_ref": self.operation_ref,
            "status": self.status,
            "source": self.source,
        }


def _flag(value: Any) -> bool:
    return value is True


def receipt_from_report(report: dict[str, Any], *, issuer: str, **meta: Any) -> DealReceipt:
    """Build a receipt from an auditor report or a rehearsal record.

    An auditor report carries an ``auditor`` block of independent flags. A rehearsal record
    carries ``settled`` and ``audit.compliant`` from the participant's own view, which is
    agreement and payment evidence but never independent verification.
    """
    auditor = report.get("auditor")
    if isinstance(auditor, dict):
        return DealReceipt(
            deal_commitment=str(auditor.get("deal_commitment", "")),
            issuer=issuer,
            agreement_verified=_flag(auditor.get("agreement_verified")),
            payment_verified=_flag(auditor.get("payment_verified")),
            delivery_verified=_flag(auditor.get("delivery_verified")),
            operation_ref=report.get("operation_ref"),
            source="disclosure",
            **meta,
        )
    agreed = report.get("agreed_amount")
    paid = report.get("paid_amount")
    audit = report.get("audit") or {}
    return DealReceipt(
        deal_commitment=str(report.get("deal_commitment") or report.get("deal_ref") or ""),
        issuer=issuer,
        agreement_verified=bool(report.get("settled")),
        payment_verified=bool(audit.get("compliant")) and bool(report.get("settled")),
        agreed_amount=agreed if isinstance(agreed, int) else None,
        paid_amount=paid if isinstance(paid, int) else None,
        operation_ref=report.get("operation_ref"),
        source="rehearsal",
        **meta,
    )


def render_text(receipt: DealReceipt) -> str:
    def mark(flag: bool) -> str:
        return "verified" if flag else "pending"

    lines = [
        "Private deal receipt",
        "====================",
        f"Status:     {receipt.status}",
        f"Deal:       {receipt.deal_commitment}",
        f"Issuer:     {receipt.issuer}",
    ]
    if receipt.parties:
        lines.append(f"Parties:    {', '.join(receipt.parties)}")
    if receipt.operation_ref:
        lines.append(f"Operation:  {receipt.operation_ref}")
    lines += [
        "",
        f"Agreement:  {mark(receipt.agreement_verified)}",
        f"Payment:    {mark(receipt.payment_verified)}",
        "Delivery:   not proven (disclosure never proves delivery)",
        "",
    ]
    if receipt.agreed_amount is not None or receipt.paid_amount is not None:
        asset = f" {receipt.asset}" if receipt.asset else ""
        lines.append(f"Agreed:     {receipt.agreed_amount}{asset}")
        lines.append(f"Paid:       {receipt.paid_amount if receipt.paid_amount is not None else 'pending'}{asset}")
        lines.append(f"Consistent: {receipt.consistency}")
    if receipt.network:
        lines.append(f"Network:    {receipt.network}")
    lines.append(f"Source:     {receipt.source}")
    if not receipt.payment_verified:
        lines.append("")
        lines.append("Payment pending is not evidence of non-payment; re-check later, do not pay again.")
    return "\n".join(lines) + "\n"


def render_html(receipt: DealReceipt) -> str:
    def row(label: str, value: str, tone: str = "") -> str:
        klass = f' class="{tone}"' if tone else ""
        return f"<tr><th>{html.escape(label)}</th><td{klass}>{html.escape(value)}</td></tr>"

    def flag(label: str, value: bool) -> str:
        return row(label, "verified" if value else "pending", "ok" if value else "warn")

    amounts = ""
    if receipt.agreed_amount is not None or receipt.paid_amount is not None:
        amounts = (
            row("Agreed", f"{receipt.agreed_amount}{' ' + receipt.asset if receipt.asset else ''}")
            + row("Paid", f"{receipt.paid_amount if receipt.paid_amount is not None else 'pending'}"
                  f"{' ' + receipt.asset if receipt.asset else ''}")
            + row("Consistent", receipt.consistency, "ok" if receipt.consistency == "consistent" else "warn")
        )
    body = (
        row("Status", receipt.status, "ok" if receipt.status == "verified" else "warn")
        + row("Deal", receipt.deal_commitment)
        + row("Issuer", receipt.issuer)
        + (row("Parties", ", ".join(receipt.parties)) if receipt.parties else "")
        + (row("Operation", receipt.operation_ref) if receipt.operation_ref else "")
        + flag("Agreement", receipt.agreement_verified)
        + flag("Payment", receipt.payment_verified)
        + row("Delivery", "not proven (disclosure never proves delivery)", "muted")
        + amounts
        + (row("Network", receipt.network) if receipt.network else "")
        + row("Source", receipt.source)
    )
    note = (
        "" if receipt.payment_verified
        else '<p class="note">Payment pending is not evidence of non-payment; re-check later, do not pay again.</p>'
    )
    return (
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">"
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
        "<title>Private deal receipt</title><style>"
        "body{font:15px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;background:#0a0908;color:#e8e3dc;"
        "margin:0;padding:3rem 1.25rem}"
        "main{max-width:44rem;margin:0 auto}"
        "h1{font-size:1.1rem;letter-spacing:.02em;margin:0 0 1.5rem}"
        "table{width:100%;border-collapse:collapse}"
        "th,td{text-align:left;padding:.5rem .25rem;border-bottom:1px solid #26221d;vertical-align:top}"
        "th{width:11rem;color:#9a938a;font-weight:400}"
        ".ok{color:#7bd88f}.warn{color:#f0b429}.muted{color:#9a938a}"
        ".note{margin-top:1.5rem;color:#9a938a;font-size:13px}"
        "</style></head><body><main><h1>Private deal receipt</h1><table>"
        + body + "</table>" + note + "</main></body></html>\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True,
                        help="JSON from metropolis_audit.py or a procurement/marketplace rehearsal")
    parser.add_argument("--issuer", required=True, help="the issuing participant's identity")
    parser.add_argument("--parties", nargs="*", default=[], help="participant identities, if known")
    parser.add_argument("--asset", help="ERC-20 ticker or address")
    parser.add_argument("--network", help="network label, e.g. eip155:10143")
    parser.add_argument("--deal-commitment", help="override the commitment when the report omits it")
    parser.add_argument("--format", choices=["text", "json", "html"], default="text")
    parser.add_argument("--out", type=Path, help="write to a file instead of stdout")
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    if args.deal_commitment:
        report = {**report, "deal_commitment": args.deal_commitment}
    receipt = receipt_from_report(
        report, issuer=args.issuer, parties=tuple(args.parties), asset=args.asset, network=args.network)
    if args.format == "json":
        rendered = json.dumps(receipt.to_dict(), indent=2) + "\n"
    elif args.format == "html":
        rendered = render_html(receipt)
    else:
        rendered = render_text(receipt)
    if args.out:
        args.out.write_text(rendered)
        if args.format == "html":
            print(f"wrote {args.out}")
    else:
        sys.stdout.write(rendered)


if __name__ == "__main__":
    main()
