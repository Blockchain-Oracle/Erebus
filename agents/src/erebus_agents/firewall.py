"""Agent-side spending policy: caps, a daily ledger, and the counterparty allowlist.

This complements, and does not replace, the MCP-layer `SpendGuard`
(`mcp-server/src/erebus_mcp/spending.py`). That guard already refuses a settlement above a
per-deal or daily cap and keeps the reservation crash-safe against the chain outcome. What it
does not do is restrict *who* the agent may pay. An agent that can settle with any address can
be steered to a wrong or unknown counterparty even while staying under its amount caps.

So this module is the policy the agent applies on top: same per-token per-deal and daily
amounts, plus an allowlist of counterparties. An empty allowlist means the operator has not
restricted counterparties and every address is allowed; a non-empty allowlist means only those
addresses are. A token with no configured limit is uncapped, as in `SpendGuard`. The order is
deliberate: counterparty first, then per-deal, then daily, so the most specific refusal is the
one the agent sees.

Token and counterparty identities are compared case-insensitively, so a checksummed EVM address
in the policy matches the lowercase form a client reports, and vice versa. Without that, a
mixed-case token key would silently fall through to "uncapped".

This module is not yet wired into the settlement path: nothing calls it except
`firewall_demo.py`. An agent must call `guard` or `charge` itself before settling; until then,
`SpendGuard` is the only limit actually enforced on a settlement.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from erebus_mcp.interface import AgentId

ALLOWED = "allowed"
COUNTERPARTY_NOT_ALLOWED = "counterparty_not_allowed"
PER_DEAL_EXCEEDED = "per_deal_exceeded"
DAILY_EXCEEDED = "daily_exceeded"


def normalize_identity(value: str) -> str:
    """Canonical form for comparing token and participant identities: trimmed, lowercase."""
    return value.strip().lower()


@dataclass(frozen=True)
class TokenLimit:
    """Caps for one token. A missing field means unlimited on that axis."""

    per_deal: int | None = None
    daily: int | None = None


@dataclass(frozen=True)
class SpendingPolicy:
    """An operator's spend rules for one agent identity.

    ``allowed_counterparties`` is a set of participant identities. Empty means unrestricted.
    """

    by_token: dict[str, TokenLimit] = field(default_factory=dict)
    allowed_counterparties: frozenset[AgentId] = frozenset()

    def __post_init__(self) -> None:
        by_token: dict[str, TokenLimit] = {}
        for token, limit in self.by_token.items():
            key = normalize_identity(token)
            if not key:
                raise ValueError("token keys must not be empty")
            if key in by_token and by_token[key] != limit:
                raise ValueError(f"token {token!r} is configured twice with different limits")
            by_token[key] = limit
        object.__setattr__(self, "by_token", by_token)
        object.__setattr__(self, "allowed_counterparties",
                           frozenset(normalize_identity(party) for party in self.allowed_counterparties))

    def limit_for(self, token: str) -> TokenLimit:
        return self.by_token.get(normalize_identity(token), TokenLimit())

    def allows_counterparty(self, counterparty: AgentId) -> bool:
        return not self.allowed_counterparties or normalize_identity(counterparty) in self.allowed_counterparties


@dataclass(frozen=True)
class FirewallDecision:
    allowed: bool
    code: str
    message: str


class SpendLedger:
    """A day-keyed, in-memory record of committed spend for one agent.

    The MCP `SpendGuard` owns the durable, chain-authoritative ledger for settlement. This one
    is what an agent keeps while planning, so it can refuse a deal before it even calls the
    server.
    """

    def __init__(self) -> None:
        self._spent: dict[tuple[str, str], int] = {}

    def spent(self, day: str, token: str) -> int:
        return self._spent.get((day, normalize_identity(token)), 0)

    def record(self, day: str, token: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError("amount must be positive")
        key = (day, normalize_identity(token))
        self._spent[key] = self._spent.get(key, 0) + amount


def evaluate(policy: SpendingPolicy, *, token: str, amount: int, counterparty: AgentId,
             committed_today: int = 0) -> FirewallDecision:
    if amount <= 0:
        raise ValueError("amount must be positive")
    if not token:
        raise ValueError("token must not be empty")
    if not policy.allows_counterparty(counterparty):
        return FirewallDecision(False, COUNTERPARTY_NOT_ALLOWED,
                                f"counterparty {counterparty!r} is not on the allowlist")
    limit = policy.limit_for(token)
    if limit.per_deal is not None and amount > limit.per_deal:
        return FirewallDecision(False, PER_DEAL_EXCEEDED,
                                f"{amount} exceeds the per-deal cap {limit.per_deal} for {token}")
    if limit.daily is not None and committed_today + amount > limit.daily:
        return FirewallDecision(False, DAILY_EXCEEDED,
                                f"{committed_today + amount} would exceed the daily cap {limit.daily} for {token}")
    return FirewallDecision(True, ALLOWED, "within policy")


def guard(policy: SpendingPolicy, ledger: SpendLedger, *, token: str, amount: int,
          counterparty: AgentId, day: str) -> FirewallDecision:
    """Check one intended spend against the policy and this agent's committed spend today."""
    return evaluate(policy, token=token, amount=amount, counterparty=counterparty,
                    committed_today=ledger.spent(day, token))


def charge(policy: SpendingPolicy, ledger: SpendLedger, *, token: str, amount: int,
           counterparty: AgentId, day: str) -> FirewallDecision:
    """Check, and commit to the ledger only when allowed."""
    decision = guard(policy, ledger, token=token, amount=amount, counterparty=counterparty, day=day)
    if decision.allowed:
        ledger.record(day, token, amount)
    return decision
