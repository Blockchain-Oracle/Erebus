"""Grant-funded agent work: a sponsor gives an agent a budget and the rules to spend it under.

A `BudgetGrant` is what a third party (a sponsor) authorizes: a token, a total budget, an
optional per-deal cap, an allowlist of counterparties, and an expiry. The agent draws on it and
the grant refuses anything outside those rules. This reuses the same decision vocabulary as the
spending firewall (`firewall.py`) so an operator reads one set of refusal codes across both.

The grant is policy, not custody: it holds no keys and moves no value. The agent's own wallet
and the MCP `SpendGuard` still enforce the durable, chain-authoritative limits; the grant is
what makes the agent's spending attributable to a sponsor's intent. `GrantLedger` is the local
record of what has been drawn so far, for the compliance summary a sponsor can later review.
"""

from __future__ import annotations

from dataclasses import dataclass

from erebus_mcp.interface import AgentId

from erebus_agents.firewall import ALLOWED, COUNTERPARTY_NOT_ALLOWED, PER_DEAL_EXCEEDED, FirewallDecision

GRANT_EXPIRED = "grant_expired"
GRANT_EXHAUSTED = "grant_exhausted"


@dataclass(frozen=True)
class BudgetGrant:
    grant_id: str
    sponsor: AgentId
    agent: AgentId
    token: str
    max_total: int
    expires_at: int
    per_deal: int | None = None
    allowed_counterparties: frozenset[AgentId] = frozenset()

    def __post_init__(self) -> None:
        if not self.grant_id or not self.sponsor or not self.agent or not self.token:
            raise ValueError("grant_id, sponsor, agent and token must not be empty")
        if self.max_total <= 0:
            raise ValueError("max_total must be positive")
        if self.per_deal is not None and self.per_deal <= 0:
            raise ValueError("per_deal must be positive when present")
        if self.expires_at <= 0:
            raise ValueError("expires_at must be a positive unix time")


class GrantLedger:
    """Local record of what has been drawn from each grant."""

    def __init__(self) -> None:
        self._spent: dict[str, int] = {}

    def spent(self, grant_id: str) -> int:
        return self._spent.get(grant_id, 0)

    def remaining(self, grant: BudgetGrant) -> int:
        return grant.max_total - self.spent(grant.grant_id)

    def record(self, grant_id: str, amount: int) -> None:
        self._spent[grant_id] = self._spent.get(grant_id, 0) + amount


def authorize(grant: BudgetGrant, ledger: GrantLedger, *, amount: int, counterparty: AgentId,
              at: int) -> FirewallDecision:
    if amount <= 0:
        raise ValueError("amount must be positive")
    if at > grant.expires_at:
        return FirewallDecision(False, GRANT_EXPIRED, f"grant expired at {grant.expires_at}")
    if grant.allowed_counterparties and counterparty not in grant.allowed_counterparties:
        return FirewallDecision(False, COUNTERPARTY_NOT_ALLOWED,
                                f"counterparty {counterparty!r} is not on the grant's allowlist")
    if grant.per_deal is not None and amount > grant.per_deal:
        return FirewallDecision(False, PER_DEAL_EXCEEDED,
                                f"{amount} exceeds the grant's per-deal cap {grant.per_deal}")
    if ledger.spent(grant.grant_id) + amount > grant.max_total:
        return FirewallDecision(False, GRANT_EXHAUSTED,
                                f"{ledger.spent(grant.grant_id) + amount} would exceed the budget {grant.max_total}")
    return FirewallDecision(True, ALLOWED, "within the grant")


def charge(grant: BudgetGrant, ledger: GrantLedger, *, amount: int, counterparty: AgentId,
           at: int) -> FirewallDecision:
    decision = authorize(grant, ledger, amount=amount, counterparty=counterparty, at=at)
    if decision.allowed:
        ledger.record(grant.grant_id, amount)
    return decision


@dataclass(frozen=True)
class GrantSummary:
    grant_id: str
    sponsor: AgentId
    agent: AgentId
    spent: int
    max_total: int
    draws: int = 0

    @property
    def remaining(self) -> int:
        return self.max_total - self.spent


def summarize(grant: BudgetGrant, ledger: GrantLedger, *, draws: int = 0) -> GrantSummary:
    return GrantSummary(grant.grant_id, grant.sponsor, grant.agent, ledger.spent(grant.grant_id),
                        grant.max_total, draws)
