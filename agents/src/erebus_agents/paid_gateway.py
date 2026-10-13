"""A deterministic, offline model of the `x402-exact` seller gateway.

The production rail is the installed `erebus-access-service`, reached through the buyer's
access MCP server with the `x402-exact` profile. This module models the *sequencing* those
pieces implement so a demo can exercise the real production driver, `drive_x402_deal`, without
a chain: negotiate, then retrieve. The first retrieval is the payment request -- the buyer
signs one Permit2 authorization and the seller submits it -- and every retry is another
retrieval that reuses the retained permit. Nothing here reports independent payment or
delivery verification, exactly like the real path, because a verified resource hash and the
seller's finality claim are not chain evidence.

This is a rehearsal transport, not a settlement. Use `erebus-access-service` for the real rail.
Standard x402 is public; never present it as private (docs/metropolis-roadmap.md).
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class GatewayEndpoint:
    """One paid resource behind the gateway: a capability, a price, and an asset."""

    service_id: str
    capability: str
    price: int
    asset: str
    path: str = "/v1/resource"

    def __post_init__(self) -> None:
        if not self.service_id or not self.capability or not self.asset:
            raise ValueError("service_id, capability and asset must not be empty")
        if self.price <= 0:
            raise ValueError("price must be positive")


@dataclass
class _Operation:
    permit_signed: bool = False
    payment_submitted: bool = False
    served: bool = False
    attempts: int = 0


@dataclass
class LocalGateway:
    """Shared seller state for one or more paid calls, exposed as buyer and seller sessions.

    `pending_states` is the sequence of `status` values a retrieval returns before it is
    served. The defaults model a submitted payment and one dropped response, so a demo shows
    that retries reuse the permit instead of paying again.

    The permit and payment counters are this model's own bookkeeping, so they show what the
    model does, not what the driver does. `negotiate_calls` is the driver-side signal: retries
    must reach the seller as retrievals only, never as a fresh negotiation.
    """

    endpoint: GatewayEndpoint
    resource: bytes = b"paid resource body"
    pending_states: tuple[str, ...] = ("payment_pending", "paid_but_undelivered")
    _operations: dict[str, _Operation] = field(default_factory=dict)
    permits_signed: int = 0
    payments_collected: int = 0
    negotiate_calls: int = 0
    retrieve_calls: int = 0

    def _operation(self, operation_ref: str) -> _Operation:
        return self._operations.setdefault(operation_ref, _Operation())

    def _commitment(self, operation_ref: str) -> str:
        return hashlib.sha256(f"{self.endpoint.service_id}:{operation_ref}".encode()).hexdigest()

    def buyer_session(self) -> "GatewaySession":
        return GatewaySession(self, "buyer")

    def seller_session(self) -> "GatewaySession":
        return GatewaySession(self, "seller")

    def _negotiate(self, operation_ref: str) -> dict[str, Any]:
        self.negotiate_calls += 1
        return {"ok": True, "result": {
            "deal_commitment": self._commitment(operation_ref),
            "evidence_file": f"/operator/state/agent/{operation_ref}.0.tx",
        }}

    def _retrieve(self, operation_ref: str) -> dict[str, Any]:
        self.retrieve_calls += 1
        operation = self._operation(operation_ref)
        if not operation.permit_signed:
            operation.permit_signed = True
            self.permits_signed += 1
        if not operation.payment_submitted:
            operation.payment_submitted = True
            self.payments_collected += 1
        attempt = operation.attempts
        operation.attempts += 1
        if attempt < len(self.pending_states):
            return {"ok": False, "result": {"status": self.pending_states[attempt],
                                            "retry_without_payment": True, "payment_verified": False}}
        operation.served = True
        return {"ok": True, "result": {"status": "retrieved", "result": {
            "resource_verified": True,
            "payment_verified": False,
            "delivery_verified": False,
            "resource_sha256": hashlib.sha256(self.resource).hexdigest(),
            "resource_bytes": len(self.resource),
            "seller_reported_payment_finalized": True,
        }}}


class GatewaySession:
    """One side's view of `LocalGateway`, satisfying the `ToolSession` protocol."""

    def __init__(self, gateway: LocalGateway, side: str) -> None:
        self._gateway = gateway
        self._side = side

    async def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        if name == "negotiate_deal":
            return self._gateway._negotiate(arguments["operation_ref"])
        if name == "retrieve_service_access" and self._side == "buyer":
            operation_ref = arguments["evidence_name"].rsplit(".", 2)[0]
            return self._gateway._retrieve(operation_ref)
        raise ValueError(f"{self._side} session has no tool {name!r}")
