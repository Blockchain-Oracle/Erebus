"""Pay per call for an API over the operator-selected `x402-exact` rail.

Each call is one deal. The buyer's x402 server exposes only `negotiate_deal` and
`retrieve_service_access`: both sides negotiate, and the first retrieval signs one local Permit2
authorization that the seller submits. Every retry is another retrieval that reuses the retained
permit, so a dropped response can never pay twice. This runs the production sequence
`metropolis_loop.drive_x402_deal` over the participants' real MCP servers, with the buyer's
access settings (`EREBUS_ACCESS_*`, including `EREBUS_ACCESS_PAYMENT_RAIL=x402-exact`) fixed by
its operator and the seller's `erebus-access-service` behind them.

The operation IDs for every call are written to an owner-only ledger before the first call. An
interrupted run is resumed by pointing at the same ledger: finished calls are retrieved again
(reusing their permits), and no call ever gets a fresh ID.

Nothing here reports independent payment or delivery verification: a verified resource hash and
the seller's finality claim are not chain evidence. An independent auditor verifies payment from
the disclosure grant (`metropolis_audit.py`). Standard x402 is public, not private: accepted
terms and identities are visible.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import secrets
import sys
from pathlib import Path
from typing import Any, Sequence

from erebus_agents.marketplace import write_private_json
from erebus_agents.metropolis_loop import (
    X402_BUYER_TOOLS,
    HarnessError,
    ToolSession,
    _harness_error,
    connect_participants,
    drive_x402_deal,
    server_params,
)


def load_or_create_ledger(path: Path, calls: int) -> list[str]:
    """Return the operation IDs for `calls` paid calls, creating and persisting them only once."""
    if calls <= 0:
        raise ValueError("calls must be positive")
    if path.exists():
        refs = json.loads(path.read_text()).get("operation_refs")
        if not isinstance(refs, list) or len(refs) != calls or not all(isinstance(ref, str) for ref in refs):
            raise ValueError("the existing ledger does not hold this many calls; resume it with its own count")
        return refs
    refs = [secrets.token_hex(32) for _ in range(calls)]
    write_private_json(path, {"operation_refs": refs})
    return refs


async def run_calls(buyer: ToolSession, seller: ToolSession, operation_refs: Sequence[str],
                    **options: Any) -> dict[str, Any]:
    """Drive each call in order; stop at the first failure and report what is left to resume."""
    calls: list[dict[str, Any]] = []
    for index, operation_ref in enumerate(operation_refs):
        try:
            record = await drive_x402_deal(buyer, seller, operation_ref, **options)
        except HarnessError as error:
            return {"rail": "x402-exact", "calls": calls, "completed": len(calls),
                    "failed": {"operation_ref": operation_ref, "error": str(error)},
                    "remaining": list(operation_refs[index:]), "payment_verified": False,
                    "payment_verification": "independent auditor only"}
        calls.append({key: record[key] for key in (
            "operation_ref", "deal_commitment", "retrieval_attempts", "retrieval_states", "resource_sha256",
            "resource_bytes", "seller_reported_payment_finalized")})
    return {"rail": "x402-exact", "calls": calls, "completed": len(calls), "failed": None, "remaining": [],
            "payment_verified": False, "payment_verification": "independent auditor only",
            "rails_note": "standard x402 is public, not private"}


async def pay_per_call(buyer_negotiation: str, buyer_access: dict[str, str], seller_negotiation: str,
                       operation_refs: Sequence[str], *, server_command: str | None = None,
                       negotiation_cli: str | None = None, max_polls: int = 120) -> dict[str, Any]:
    if buyer_access.get("EREBUS_ACCESS_PAYMENT_RAIL") != "x402-exact":
        raise ValueError("the buyer operator must select EREBUS_ACCESS_PAYMENT_RAIL=x402-exact")
    buyer = server_params(sys.executable, buyer_negotiation, access=buyer_access, server_command=server_command,
                          negotiation_cli=negotiation_cli)
    seller = server_params(sys.executable, seller_negotiation, negotiation_cli=negotiation_cli,
                           server_command=server_command)
    async with connect_participants(buyer, seller, buyer_allowed=(X402_BUYER_TOOLS,)) as ((b, _), (s, _)):
        return await run_calls(b, s, operation_refs, max_polls=max_polls)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--buyer-negotiation", required=True)
    parser.add_argument("--buyer-access", type=Path, required=True,
                        help="JSON file of the buyer operator's EREBUS_ACCESS_* variables (x402-exact rail)")
    parser.add_argument("--seller-negotiation", required=True)
    parser.add_argument("--calls", type=int, required=True, help="independent paid calls to run")
    parser.add_argument("--ledger", type=Path, required=True,
                        help="owner-only file holding this run's operation IDs; reuse it to resume")
    parser.add_argument("--server-command", help="installed erebus-mcp-server; default: this Python's module")
    parser.add_argument("--negotiation-cli")
    parser.add_argument("--max-polls", type=int, default=120)
    args = parser.parse_args()
    refs = load_or_create_ledger(args.ledger, args.calls)
    try:
        record = asyncio.run(pay_per_call(args.buyer_negotiation, json.loads(args.buyer_access.read_text()),
                                          args.seller_negotiation, refs, server_command=args.server_command,
                                          negotiation_cli=args.negotiation_cli, max_polls=args.max_polls))
    except BaseException as error:
        found = _harness_error(error)
        if found is None:
            raise
        print(json.dumps({"payment_verified": False, "error": str(found), "ledger": str(args.ledger)}))
        raise SystemExit(1) from None
    json.dump(record, sys.stdout, indent=2)
    sys.stdout.write("\n")
    if record["failed"] is not None:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
