"""Autonomous vendor selection: negotiate privately with several sellers, then pay exactly one.

The buyer holds one request and several candidate sellers. It runs a private negotiation with
each candidate and records the amount each would agree to, without settling. It then selects the
cheapest agreement at or below its ceiling and pays that seller once. Losing candidates learn
nothing about each other, and only the winner's identity reaches the settlement path.

`negotiate_amount` mirrors the buyer/seller loop in `agent.run_negotiation` but stops at
agreement instead of calling `accept_and_settle`, so a losing candidate is never paid. The
selection is a pure function of the outcomes.
"""

from __future__ import annotations

from dataclasses import dataclass

from erebus_mcp.interface import AgentId, ChannelState
from erebus_mcp.mock_client import MockErebusClient

from erebus_agents.policy import BuyerPolicy, NegotiationAction, SellerPolicy


@dataclass(frozen=True)
class VendorOutcome:
    service_id: str
    seller: AgentId
    settled: bool
    agreed_amount: int | None

    def __post_init__(self) -> None:
        if not self.service_id or not self.seller:
            raise ValueError("service_id and seller must not be empty")
        if self.settled != (self.agreed_amount is not None):
            raise ValueError("a settled outcome carries an agreed amount")


@dataclass(frozen=True)
class Selection:
    winner: VendorOutcome | None
    rejected: tuple[tuple[str, str], ...]

    @property
    def awarded(self) -> bool:
        return self.winner is not None


def select(outcomes: tuple[VendorOutcome, ...], *, ceiling: int) -> Selection:
    """Cheapest settled agreement at or below the ceiling; ties break on seller identity."""
    valid: list[VendorOutcome] = []
    rejected: list[tuple[str, str]] = []
    for outcome in outcomes:
        if not outcome.settled or outcome.agreed_amount is None:
            rejected.append((outcome.service_id, "no agreement"))
        elif outcome.agreed_amount > ceiling:
            rejected.append((outcome.service_id, "above the buyer's ceiling"))
        else:
            valid.append(outcome)
    if not valid:
        return Selection(None, tuple(rejected))
    winner = min(valid, key=lambda outcome: (outcome.agreed_amount, outcome.seller))  # type: ignore[arg-type]
    losers = tuple((outcome.service_id, "outbid") for outcome in valid if outcome is not winner)
    return Selection(winner, tuple(rejected) + losers)


async def negotiate_amount(
    *,
    buyer_client: MockErebusClient,
    seller_client: MockErebusClient,
    buyer_address: AgentId,
    seller_address: AgentId,
    token: str,
    buyer_policy: BuyerPolicy,
    seller_policy: SellerPolicy,
    max_rounds: int = 3,
) -> int | None:
    """Return the amount the buyer would accept, or None, without settling anything."""
    buyer_handle = await buyer_client.open_channel(seller_address)
    seller_handle = await seller_client.open_channel(buyer_address)
    for round_index in range(max_rounds + 1):
        state: ChannelState = await buyer_client.read_channel_state(buyer_handle)
        balance = await buyer_client.note_balance()
        decision = buyer_policy.decide(state, round_index, token, balance.total)
        if decision.action is NegotiationAction.WALK:
            return None
        if decision.action is NegotiationAction.ACCEPT:
            accepted = next(offer for offer in state.offers if offer.offer_id == decision.reply_to)
            return accepted.terms.amount
        if decision.action is NegotiationAction.PROPOSE:
            await buyer_client.propose_offer(buyer_handle, decision.terms)
        else:
            await buyer_client.counter_offer(buyer_handle, decision.reply_to, decision.terms)

        seller_state = await seller_client.read_channel_state(seller_handle)
        seller_decision = seller_policy.decide(seller_state, round_index)
        if seller_decision.action is NegotiationAction.WALK:
            return None
        if seller_decision.action is NegotiationAction.COUNTER:
            await seller_client.counter_offer(seller_handle, seller_decision.reply_to, seller_decision.terms)
    return None
