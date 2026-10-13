"""Private SaaS metering: buy a bundle of calls in one settlement, then spend the calls.

The buyer's Metropolis server is configured by its operator with the metered access profile
(`EREBUS_ACCESS_PROFILE=metered` among its `EREBUS_ACCESS_*` settings), so it exposes the ordinary
settlement tools plus `metered_service_status` and `call_metered_service`. One run settles the
bundle through `metropolis_loop.drive_deal` (negotiate on both sides, settle at most once,
recover by observation until finalized), then spends calls against the seller's real allowance
and refuses to overdraw it. Payment is the settlement; usage is the seller's metered issuer. An
auditor verifies the payment from the disclosure grant, never the call count.

The bundle size and price are fixed by the operators' files, not chosen here. Reuse the same
`--operation` after any interruption. To spend more calls from a bundle already paid under that
operation, pass `--consume-only`: it never touches the settlement tools.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from pathlib import Path
from typing import Any

from erebus_agents.metering import METERED_TOOLS, consume
from erebus_agents.metropolis_loop import (
    BUYER_TOOLS,
    ToolSession,
    _harness_error,
    connect_participants,
    drive_deal,
    server_params,
)


def evidence_name(operation_ref: str) -> str:
    """Negotiation writes the buyer's evidence as `<state_root>/agent/<operation>.0.tx`."""
    return f"{operation_ref}.0.tx"


async def buy_and_consume(buyer: ToolSession, seller: ToolSession, operation_ref: str, calls: int, *,
                          consume_only: bool = False, **options: Any) -> dict[str, Any]:
    record: dict[str, Any] = {"operation_ref": operation_ref}
    if not consume_only:
        deal = await drive_deal(buyer, seller, operation_ref, **options)
        record.update(deal_commitment=deal["deal_commitment"], settlement_payment_verified=deal["payment_verified"],
                      settle_calls=deal["settle_calls"], recover_calls=deal["recover_calls"])
    record["usage"] = await consume(buyer, evidence_name(operation_ref), calls, **options)
    return record


async def run(buyer_negotiation: str, buyer_payment: str, buyer_access: dict[str, str], seller_negotiation: str,
              operation_ref: str, calls: int, *, consume_only: bool = False, server_command: str | None = None,
              negotiation_cli: str | None = None, payment_cli: str | None = None,
              max_polls: int = 120) -> dict[str, Any]:
    if buyer_access.get("EREBUS_ACCESS_PROFILE") != "metered":
        raise ValueError("the buyer operator must select EREBUS_ACCESS_PROFILE=metered")
    buyer = server_params(sys.executable, buyer_negotiation, payment_config=buyer_payment, access=buyer_access,
                          server_command=server_command, negotiation_cli=negotiation_cli, payment_cli=payment_cli)
    seller = server_params(sys.executable, seller_negotiation, negotiation_cli=negotiation_cli,
                           server_command=server_command)
    async with connect_participants(buyer, seller, buyer_allowed=(BUYER_TOOLS | METERED_TOOLS,)) as ((b, _), (s, _)):
        return await buy_and_consume(b, s, operation_ref, calls, consume_only=consume_only, max_polls=max_polls)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--buyer-negotiation", required=True)
    parser.add_argument("--buyer-payment", required=True)
    parser.add_argument("--buyer-access", type=Path, required=True,
                        help="JSON file of the buyer operator's EREBUS_ACCESS_* variables (metered profile)")
    parser.add_argument("--seller-negotiation", required=True)
    parser.add_argument("--operation", required=True, help="64-hex operation ID; reuse it after any interruption")
    parser.add_argument("--calls", type=int, required=True, help="calls to spend from the bundle")
    parser.add_argument("--consume-only", action="store_true", help="the bundle is already paid; only spend calls")
    parser.add_argument("--server-command", help="installed erebus-mcp-server; default: this Python's module")
    parser.add_argument("--negotiation-cli")
    parser.add_argument("--payment-cli")
    parser.add_argument("--max-polls", type=int, default=120)
    args = parser.parse_args()
    try:
        record = asyncio.run(run(args.buyer_negotiation, args.buyer_payment, json.loads(args.buyer_access.read_text()),
                                 args.seller_negotiation, args.operation, args.calls, consume_only=args.consume_only,
                                 server_command=args.server_command, negotiation_cli=args.negotiation_cli,
                                 payment_cli=args.payment_cli, max_polls=args.max_polls))
    except BaseException as error:
        found = _harness_error(error)
        if found is None:
            raise
        print(json.dumps({"operation_ref": args.operation, "error": str(found)}))
        raise SystemExit(1) from None
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
