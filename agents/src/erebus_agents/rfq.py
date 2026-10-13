"""A confidential request-for-quote desk: one buyer, many suppliers, one award.

A buyer publishes a `RequestForQuote` with a hidden ceiling. Suppliers return `Quote`s. The
award picks the cheapest valid quote without revealing the losing bids: only the winner's
identity and price are used downstream, and the rejected quotes stay in the buyer's local
record. The negotiation and settlement still go through the ordinary loop; this module owns
only the comparison. The awarded quote is binding: the buyer settles at or below
`Award.awarded_total`, never at a price the winner did not quote (see `award_honored`).

Quotes are sealed by construction here (each supplier only sees the request), not by the
protocol. On the real rail the request would travel over the encrypted channel; this module
never sees key material and does no I/O.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from erebus_mcp.interface import AgentId


@dataclass(frozen=True)
class RequestForQuote:
    rfq_id: str
    item: str
    quantity: int
    max_unit_price: int
    deadline: int

    def __post_init__(self) -> None:
        if not self.rfq_id or not self.item:
            raise ValueError("rfq_id and item must not be empty")
        if self.quantity <= 0 or self.max_unit_price <= 0:
            raise ValueError("quantity and max_unit_price must be positive")
        if self.deadline <= 0:
            raise ValueError("deadline must be a positive unix time")

    @property
    def ceiling(self) -> int:
        return self.quantity * self.max_unit_price


@dataclass(frozen=True)
class Quote:
    supplier: AgentId
    rfq_id: str
    unit_price: int
    valid_until: int

    def __post_init__(self) -> None:
        if not self.supplier or not self.rfq_id:
            raise ValueError("supplier and rfq_id must not be empty")
        if self.unit_price <= 0:
            raise ValueError("unit_price must be positive")
        if self.valid_until <= 0:
            raise ValueError("valid_until must be a positive unix time")

    def total(self, quantity: int) -> int:
        return self.unit_price * quantity


class AwardStatus(str, Enum):
    AWARDED = "awarded"
    NO_VALID_QUOTE = "no_valid_quote"
    REQUEST_CLOSED = "request_closed"


@dataclass(frozen=True)
class Award:
    status: AwardStatus
    winner: Quote | None
    rejected: tuple[tuple[AgentId, str], ...] = ()
    awarded_total: int | None = None

    def __post_init__(self) -> None:
        awarded = self.status is AwardStatus.AWARDED
        if awarded != (self.winner is not None):
            raise ValueError("an award carries a winner only when it is AWARDED")
        if awarded != (self.awarded_total is not None):
            raise ValueError("an award carries an awarded total only when it is AWARDED")


def _rejection(quote: Quote, rfq: RequestForQuote, at: int) -> str | None:
    if quote.rfq_id != rfq.rfq_id:
        return "quote is for another request"
    if quote.valid_until < at:
        return "quote expired"
    if quote.unit_price > rfq.max_unit_price:
        return "unit price above the buyer's ceiling"
    return None


def award(rfq: RequestForQuote, quotes: tuple[Quote, ...], *, at: int) -> Award:
    """Pick the cheapest valid quote; ties break on supplier address for determinism.

    Nothing is awarded once the request's own deadline has passed.
    """
    if at > rfq.deadline:
        return Award(AwardStatus.REQUEST_CLOSED, None,
                     tuple((quote.supplier, "request deadline passed") for quote in quotes))
    valid: list[Quote] = []
    rejected: list[tuple[AgentId, str]] = []
    for quote in quotes:
        reason = _rejection(quote, rfq, at)
        if reason is None:
            valid.append(quote)
        else:
            rejected.append((quote.supplier, reason))
    if not valid:
        return Award(AwardStatus.NO_VALID_QUOTE, None, tuple(rejected))
    winner = min(valid, key=lambda quote: (quote.unit_price, quote.supplier))
    losers = tuple((quote.supplier, "outbid") for quote in valid if quote is not winner)
    return Award(AwardStatus.AWARDED, winner, tuple(rejected) + losers, winner.total(rfq.quantity))


def award_honored(result: Award, paid_amount: int | None) -> bool:
    """True only when a payment exists and does not exceed what the winner quoted."""
    return (result.status is AwardStatus.AWARDED and result.awarded_total is not None
            and paid_amount is not None and paid_amount <= result.awarded_total)
