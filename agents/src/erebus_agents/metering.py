"""Prepaid metered access at the agent layer: one settlement buys N calls to a resource.

The protocol side already exists: Rust's metered profile
(`sdk/shielded/src/access/metered.rs` and `access/metered/{client,http,issuer}`) derives a
per-call request digest, keeps entitlement and call records, and lets an issuer certify that a
finalized settlement bought N units. `erebus-access-service` serves that profile.

This module is only the credit accounting an agent keeps while consuming a bundle: how many
calls were bought, how many remain, and a refusal once the bundle is spent. It holds no
crypto, no ledger on disk, and no chain access, so it cannot disagree with the Rust issuer; the
issuer is authoritative and this is a local projection.
"""

from __future__ import annotations

from dataclasses import dataclass

from erebus_mcp.interface import AgentId

UNIT = "call"


@dataclass(frozen=True)
class CreditBundle:
    """A buyer's prepaid calls to one seller's resource, bought by one settlement."""

    entitlement_id: str
    seller: AgentId
    buyer: AgentId
    calls_total: int
    calls_used: int = 0

    def __post_init__(self) -> None:
        if not self.entitlement_id or not self.seller or not self.buyer:
            raise ValueError("entitlement_id, seller and buyer must not be empty")
        if self.calls_total <= 0:
            raise ValueError("calls_total must be positive")
        if not 0 <= self.calls_used <= self.calls_total:
            raise ValueError("calls_used must be within [0, calls_total]")

    @property
    def remaining(self) -> int:
        return self.calls_total - self.calls_used

    @property
    def exhausted(self) -> bool:
        return self.remaining == 0

    def to_dict(self) -> dict[str, object]:
        return {"entitlement_id": self.entitlement_id, "seller": self.seller, "buyer": self.buyer,
                "unit": UNIT, "calls_total": self.calls_total, "calls_used": self.calls_used,
                "remaining": self.remaining, "exhausted": self.exhausted}


class BundleExhausted(RuntimeError):
    """The bundle has no calls left; buy another bundle rather than overdrawing this one."""


def consume(bundle: CreditBundle, calls: int = 1) -> CreditBundle:
    if calls <= 0:
        raise ValueError("calls must be positive")
    if calls > bundle.remaining:
        raise BundleExhausted(
            f"{calls} requested with {bundle.remaining} of {bundle.calls_total} remaining")
    return CreditBundle(bundle.entitlement_id, bundle.seller, bundle.buyer,
                        bundle.calls_total, bundle.calls_used + calls)
