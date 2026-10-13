"""Prepaid metered access: one settlement buys N calls, then the agent spends them one by one.

The protocol side is Rust's metered profile (`sdk/shielded/src/access/metered.rs` and
`access/metered/{client,http,issuer}`), served by `erebus-access-service` and reached through the
buyer's MCP tools `metered_service_status` and `call_metered_service` (operator-selected with
`EREBUS_ACCESS_PROFILE=metered`). Rust keeps the durable call cursor, signs each call with the
buyer's agreement key, and retries the same in-flight index after a timeout, a pending payment or
a lost reply. The seller's issuer is the authority on how many calls the settlement bought.

This module only sequences those tools and keeps the agent from overdrawing: it reads the real
allowance, consumes calls while any remain, retries a pending call by calling again (never by
paying), and stops at exhaustion. It holds no keys, no cursor of its own, and no chain access.
`payment_verified` stays false here: these tools do not observe the chain.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any, Awaitable, Callable

from erebus_agents.metropolis_loop import HarnessError, ToolSession, _structured

STATUS_TOOL = "metered_service_status"
CALL_TOOL = "call_metered_service"
METERED_TOOLS = {STATUS_TOOL, CALL_TOOL}


@dataclass(frozen=True)
class Allowance:
    """The seller-reported allowance for one entitlement, as `metered_service_status` returns it."""

    entitlement_id: str
    quantity: int
    remaining: int
    next_call_index: int
    in_flight: int | None
    exhausted: bool

    @classmethod
    def from_result(cls, result: dict[str, Any]) -> "Allowance":
        return cls(str(result["entitlement_id"]), int(result["quantity"]), int(result["remaining"]),
                   int(result["next_call_index"]), result.get("in_flight"), result["exhausted"] is True)

    def to_dict(self) -> dict[str, Any]:
        return {"entitlement_id": self.entitlement_id, "quantity": self.quantity, "remaining": self.remaining,
                "next_call_index": self.next_call_index, "in_flight": self.in_flight, "exhausted": self.exhausted}


class BundleExhausted(RuntimeError):
    """The bundle has no calls left; buy another bundle rather than overdrawing this one."""


async def _until_ok(session: ToolSession, name: str, arguments: dict[str, Any], *, poll_seconds: float,
                    max_polls: int, sleep: Callable[[float], Awaitable[None]]) -> dict[str, Any]:
    """Call a metered tool until it answers; a pending or failed reply is retried by the same call."""
    for attempt in range(max_polls):
        reply = _structured(await session.call_tool(name, arguments))
        if reply.get("ok"):
            return reply["result"]["result"]
        failure = reply.get("result") or reply.get("error") or {}
        if failure.get("retry_without_payment") is not True:
            raise HarnessError(f"{name} reported a non-retryable failure")
        if attempt + 1 < max_polls:
            await sleep(poll_seconds)
    raise HarnessError(f"{name} did not answer within the polling budget; call again later, do not pay again")


async def allowance(buyer: ToolSession, evidence_name: str, *, poll_seconds: float = 1.0, max_polls: int = 120,
                    sleep: Callable[[float], Awaitable[None]] = asyncio.sleep) -> Allowance:
    result = await _until_ok(buyer, STATUS_TOOL, {"evidence_name": evidence_name}, poll_seconds=poll_seconds,
                             max_polls=max_polls, sleep=sleep)
    return Allowance.from_result(result)


async def consume(buyer: ToolSession, evidence_name: str, calls: int, *, poll_seconds: float = 1.0,
                  max_polls: int = 120, sleep: Callable[[float], Awaitable[None]] = asyncio.sleep) -> dict[str, Any]:
    """Spend up to `calls` prepaid calls; refuse the rest once the real allowance is exhausted."""
    if calls <= 0:
        raise ValueError("calls must be positive")
    options = {"poll_seconds": poll_seconds, "max_polls": max_polls, "sleep": sleep}
    before = await allowance(buyer, evidence_name, **options)
    issued: list[dict[str, Any]] = []
    refused: str | None = None
    remaining = before.remaining
    for _ in range(calls):
        if remaining == 0:
            refused = str(BundleExhausted(f"bundle of {before.quantity} calls is exhausted; buy another bundle"))
            break
        result = await _until_ok(buyer, CALL_TOOL, {"evidence_name": evidence_name}, **options)
        if result["payment_verified"] or result["delivery_verified"]:
            raise HarnessError("metered access must not claim payment or delivery verification")
        remaining = int(result["remaining"])
        issued.append({key: result[key] for key in (
            "call_index", "remaining", "response_sha256", "response_bytes", "repeated", "late")})
    after = await allowance(buyer, evidence_name, **options)
    return {"entitlement_id": before.entitlement_id, "before": before.to_dict(), "after": after.to_dict(),
            "calls_issued": issued, "refused": refused, "payment_verified": False,
            "payment_verification": "independent auditor only"}
