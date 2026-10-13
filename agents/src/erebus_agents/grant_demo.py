"""Rehearsal of a grant-funded agent: a sponsor sets a budget and rules, the agent spends within them.

Runs a handful of intended draws against one grant and prints the decision and the resulting
compliance summary a sponsor could review.
"""

from __future__ import annotations

import json
import sys
from typing import Any

from erebus_agents.grant import BudgetGrant, GrantLedger, charge, summarize

AT = 1_000_000
ATLAS, AEGEAN, STRANGER = "0xatlas", "0xaegean", "0xstranger"


def default_grant() -> BudgetGrant:
    return BudgetGrant("grant-1", sponsor="0xsponsor", agent="0xagent", token="0xusdc",
                       max_total=1000, expires_at=AT + 3600, per_deal=400,
                       allowed_counterparties=frozenset({ATLAS, AEGEAN}))


def rehearse(grant: BudgetGrant, *, at: int = AT) -> dict[str, Any]:
    ledger = GrantLedger()
    attempts = ((ATLAS, 300), (AEGEAN, 300), (ATLAS, 300), (ATLAS, 300), (ATLAS, 500), (STRANGER, 50))
    results = []
    for counterparty, amount in attempts:
        decision = charge(grant, ledger, amount=amount, counterparty=counterparty, at=at)
        results.append({"counterparty": counterparty, "amount": amount, "allowed": decision.allowed,
                        "code": decision.code, "message": decision.message})
    expired = charge(grant, ledger, amount=10, counterparty=ATLAS, at=grant.expires_at + 1)
    summary = summarize(grant, ledger, draws=sum(1 for item in results if item["allowed"]))
    return {
        "grant": {"grant_id": grant.grant_id, "sponsor": grant.sponsor, "agent": grant.agent,
                  "max_total": grant.max_total, "per_deal": grant.per_deal,
                  "allowed_counterparties": sorted(grant.allowed_counterparties)},
        "attempts": results,
        "expired_attempt": {"allowed": expired.allowed, "code": expired.code, "message": expired.message},
        "summary": {"spent": summary.spent, "remaining": summary.remaining, "max_total": summary.max_total,
                    "draws": summary.draws},
    }


def main() -> None:
    json.dump(rehearse(default_grant()), sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
