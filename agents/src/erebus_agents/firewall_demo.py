"""Rehearsal of the agent spending firewall: caps, a daily ledger, and the counterparty allowlist.

Runs a handful of intended spends against one policy and prints the decision for each, so the
refusal reasons are visible. This is the agent-side layer; the MCP `SpendGuard` still enforces
the durable, chain-authoritative caps underneath it.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any

from erebus_agents.firewall import SpendingPolicy, SpendLedger, TokenLimit, charge

TOKEN = "0xusdc"
DAY = "2026-10-13"
ATLAS, AEGEAN, STRANGER = "0xatlas", "0xaegean", "0xstranger"


def default_policy() -> SpendingPolicy:
    return SpendingPolicy(
        by_token={TOKEN: TokenLimit(per_deal=500, daily=1000)},
        allowed_counterparties=frozenset({ATLAS, AEGEAN}),
    )


def rehearse(policy: SpendingPolicy, *, day: str = DAY) -> dict[str, Any]:
    ledger = SpendLedger()
    attempts = (
        (ATLAS, 400),
        (AEGEAN, 400),
        (ATLAS, 300),
        (STRANGER, 10),
        (AEGEAN, 600),
    )
    results = []
    for counterparty, amount in attempts:
        decision = charge(policy, ledger, token=TOKEN, amount=amount, counterparty=counterparty, day=day)
        results.append({"counterparty": counterparty, "amount": amount,
                        "allowed": decision.allowed, "code": decision.code, "message": decision.message})
    return {
        "day": day,
        "token": TOKEN,
        "policy": {"per_deal": policy.limit_for(TOKEN).per_deal, "daily": policy.limit_for(TOKEN).daily,
                   "allowed_counterparties": sorted(policy.allowed_counterparties)},
        "attempts": results,
        "committed_today": ledger.spent(day, TOKEN),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    args = parser.parse_args()
    json.dump(rehearse(default_policy()), sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
