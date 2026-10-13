"""Render one disclosed deal as a private receipt, for a terminal or a static HTML file.

The explorer does not verify anything by itself. It formats the report an independent auditor
produced with `metropolis_audit.py`, which verified the agreement (and, given a deployment, the
payment) against chain data from the issuer's disclosure grant. Only that report is accepted. Its
whole job is to keep three facts distinct and never collapse them:

* the agreement matches the disclosed deal,
* the payment matched the agreement,
* delivery was observed off-chain (never proven by the protocol).

A receipt that claims delivery verification is rejected on construction. "Payment pending" is
rendered as pending, not as failure: on a public RPC it can stay pending indefinitely, and pending
is not evidence of non-payment. Parties, asset and network are labels the operator supplies; the
auditor report does not carry them.
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
_PENDING_NOTE = "Payment pending is not evidence of non-payment; re-check later, do not pay again."


@dataclass(frozen=True)
class DealReceipt:
    deal_commitment: str
    issuer: str
    agreement_verified: bool
    payment_verified: bool
    delivery_verified: bool = False
    parties: tuple[str, ...] = ()
    asset: str | None = None
    network: str | None = None
    operation_ref: str | None = None

    def __post_init__(self) -> None:
        if not _COMMITMENT.fullmatch(self.deal_commitment):
            raise ValueError("deal_commitment must be 64 hex digits")
        if not self.issuer:
            raise ValueError("issuer must not be empty")
        if self.delivery_verified:
            raise ValueError("disclosure never proves delivery; a receipt claiming it is rejected")
        object.__setattr__(self, "deal_commitment", self.deal_commitment.lower())

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
            "asset": self.asset,
            "network": self.network,
            "operation_ref": self.operation_ref,
            "status": self.status,
        }


def receipt_from_report(report: dict[str, Any], *, issuer: str, deal_commitment: str | None = None,
                        **meta: Any) -> DealReceipt:
    """Build a receipt from a `metropolis_audit.py` report: ``{"operation_ref", "auditor": {...}}``.

    ``deal_commitment`` fills in a commitment the report omits. It never replaces one the auditor
    already verified: a different value is refused.
    """
    auditor = report.get("auditor")
    if not isinstance(auditor, dict):
        raise ValueError("expected a metropolis_audit.py report with an 'auditor' block")
    verified_commitment = auditor.get("deal_commitment")
    if (deal_commitment is not None and verified_commitment
            and str(verified_commitment).lower() != deal_commitment.lower()):
        raise ValueError("deal_commitment differs from the commitment the auditor verified")
    return DealReceipt(
        deal_commitment=str(verified_commitment or deal_commitment or ""),
        issuer=issuer,
        agreement_verified=auditor.get("agreement_verified") is True,
        payment_verified=auditor.get("payment_verified") is True,
        delivery_verified=auditor.get("delivery_verified") is True,
        operation_ref=report.get("operation_ref"),
        **meta,
    )


def _flag_text(flag: bool) -> str:
    return "verified" if flag else "pending"


def render_text(receipt: DealReceipt) -> str:
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
        f"Agreement:  {_flag_text(receipt.agreement_verified)}",
        f"Payment:    {_flag_text(receipt.payment_verified)}",
        "Delivery:   not proven (disclosure never proves delivery)",
    ]
    if receipt.asset:
        lines.append(f"Asset:      {receipt.asset}")
    if receipt.network:
        lines.append(f"Network:    {receipt.network}")
    if not receipt.payment_verified:
        lines += ["", _PENDING_NOTE]
    return "\n".join(lines) + "\n"


def render_html(receipt: DealReceipt) -> str:
    def row(label: str, value: str, tone: str = "") -> str:
        klass = f' class="{tone}"' if tone else ""
        return f"<tr><th>{html.escape(label)}</th><td{klass}>{html.escape(value)}</td></tr>"

    def flag(label: str, value: bool) -> str:
        return row(label, _flag_text(value), "ok" if value else "warn")

    body = (
        row("Status", receipt.status, "ok" if receipt.status == "verified" else "warn")
        + row("Deal", receipt.deal_commitment)
        + row("Issuer", receipt.issuer)
        + (row("Parties", ", ".join(receipt.parties)) if receipt.parties else "")
        + (row("Operation", receipt.operation_ref) if receipt.operation_ref else "")
        + flag("Agreement", receipt.agreement_verified)
        + flag("Payment", receipt.payment_verified)
        + row("Delivery", "not proven (disclosure never proves delivery)", "muted")
        + (row("Asset", receipt.asset) if receipt.asset else "")
        + (row("Network", receipt.network) if receipt.network else "")
    )
    note = "" if receipt.payment_verified else f'<p class="note">{html.escape(_PENDING_NOTE)}</p>'
    return (
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">"
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
        "<title>Private deal receipt</title><style>"
        "body{font:15px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;background:#0a0908;color:#e8e3dc;"
        "margin:0;padding:3rem 1.25rem}"
        "main{max-width:44rem;margin:0 auto}"
        "h1{font-size:1.1rem;letter-spacing:.02em;margin:0 0 1.5rem}"
        "table{width:100%;border-collapse:collapse}"
        "th,td{text-align:left;padding:.5rem .25rem;border-bottom:1px solid #26221d;vertical-align:top;"
        "overflow-wrap:anywhere}"
        "th{width:11rem;color:#9a938a;font-weight:400}"
        ".ok{color:#7bd88f}.warn{color:#f0b429}.muted{color:#9a938a}"
        ".note{margin-top:1.5rem;color:#9a938a;font-size:13px}"
        "</style></head><body><main><h1>Private deal receipt</h1><table>"
        + body + "</table>" + note + "</main></body></html>\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True, help="JSON printed by metropolis_audit.py")
    parser.add_argument("--issuer", required=True, help="the issuing participant's identity")
    parser.add_argument("--parties", nargs="*", default=[], help="participant identities, if known")
    parser.add_argument("--asset", help="ERC-20 ticker or address")
    parser.add_argument("--network", help="network label, e.g. eip155:10143")
    parser.add_argument("--deal-commitment",
                        help="fill in the commitment when the report omits it; never replaces a verified one")
    parser.add_argument("--format", choices=["text", "json", "html"], default="text")
    parser.add_argument("--out", type=Path, help="write to a file instead of stdout")
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    receipt = receipt_from_report(
        report, issuer=args.issuer, deal_commitment=args.deal_commitment,
        parties=tuple(args.parties), asset=args.asset, network=args.network)
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
