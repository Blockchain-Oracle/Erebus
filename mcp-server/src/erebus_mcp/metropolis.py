"""One participant's native negotiation, payment, and optional access MCP tools."""

from __future__ import annotations

import asyncio
import json
import os
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from erebus import MetropolisError, MetropolisSeam, MetropolisWallet, WalletPolicy
from mcp.server import MCPServer

from erebus_mcp.config import ConfigError


def _operator_file(value: str | None) -> tuple[str, dict[str, Any]]:
    if not value or not Path(value).is_absolute():
        raise ConfigError("configure absolute Metropolis operator files")
    path = Path(value)
    try:
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or not 1 <= info.st_size <= 32_768 or (os.name == "posix" and info.st_mode & 0o077):
            raise ConfigError("Metropolis configuration must be an owner-only regular file")
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        raise ConfigError("Metropolis configuration is unavailable or invalid") from None
    if not isinstance(data, dict):
        raise ConfigError("Metropolis configuration must be a JSON object")
    return str(path), data


@dataclass(frozen=True)
class MetropolisSettings:
    negotiation_config: str
    state_root: str
    role: str
    payment_config: str | None = None
    mode: str | None = None
    negotiation_binary: str | None = None
    payment_binary: str | None = None
    timeout: int = 600
    submission_mode: str = "direct"

    @classmethod
    def from_env(cls) -> MetropolisSettings:
        negotiation, data = _operator_file(os.environ.get("EREBUS_NEGOTIATION_CONFIG"))
        role, root = data.get("role"), data.get("state_root")
        if role not in {"buyer", "seller"} or not isinstance(root, str) or not Path(root).is_absolute():
            raise ConfigError("Metropolis needs a fixed buyer or seller role and absolute state root")
        payment, mode, submission_mode = None, None, "direct"
        if role == "buyer" and os.environ.get("EREBUS_ACCESS_PAYMENT_RAIL") == "x402-exact":
            if os.environ.get("EREBUS_PAYMENT_CONFIG") or not os.environ.get("EREBUS_ACCESS_SERVICE_URL"):
                raise ConfigError("x402 requires an access service and no ordinary payment configuration")
            mode = "x402-exact"
        elif role == "buyer":
            payment, selected = _operator_file(os.environ.get("EREBUS_PAYMENT_CONFIG"))
            mode = selected.get("mode", "public-bound")
            if mode not in {"public-bound", "shielded"} or selected.get("state_root") != root:
                raise ConfigError("payment mode must be explicit and share this participant's state root")
            if selected.get("relay") is not None:
                if mode != "shielded" or not isinstance(selected["relay"], dict) or selected.get("transaction_key_file") is not None:
                    raise ConfigError("sponsorship requires shielded relay configuration and no buyer gas key")
                submission_mode = "sponsored"
        elif os.environ.get("EREBUS_PAYMENT_CONFIG"):
            raise ConfigError("a seller server cannot have payment configuration")
        timeout = os.environ.get("EREBUS_NATIVE_TIMEOUT_SECONDS", "600")
        if not timeout.isascii() or not timeout.isdecimal() or not 1 <= int(timeout) <= 900:
            raise ConfigError("native timeout must be between 1 and 900 seconds")
        return cls(negotiation, root, role, payment, mode, os.environ.get("EREBUS_NEGOTIATION_CLI"),
                   os.environ.get("EREBUS_PAYMENT_CLI"), int(timeout), submission_mode)


def build_metropolis_server(settings: MetropolisSettings | None = None) -> MCPServer:
    settings = settings or MetropolisSettings.from_env()
    if settings.mode == "x402-exact" and os.environ.get("EREBUS_ACCESS_PAYMENT_RAIL") != "x402-exact":
        raise ConfigError("x402 access rail must be explicitly selected by the operator")
    try:
        seam = MetropolisSeam(**vars(settings))
        seam.version()
    except MetropolisError:
        raise ConfigError("Metropolis native binaries are unavailable or incompatible") from None
    server = MCPServer(name="erebus-metropolis", instructions=(
        "Private negotiation with operator-fixed service policy, participant keys, and settlement mode. "
        f"This participant is the {settings.role}. "
        "Reuse the same operation ID after interruption. Negotiation cannot submit a payment. "
        "Only the buyer can settle; recovery observes without submitting. "
        "Pending or failed access is not a reason to pay again. "
        "Public-bound settlement exposes the accepted terms and parties. "
        "Shielded proving uses local witnesses and authenticated public artifacts. "
        "Payment verification does not prove delivery. "
        + ("This buyer explicitly delegates submission to its operator-pinned sponsor. Remote relay status is not payment verification. No direct-submission fallback is allowed. "
           if settings.submission_mode == "sponsored" else "")
        + ("x402 exact mode exposes negotiation and access only. First retrieval signs one local permit; the seller submits payment. Retries reuse that permit; independent disclosure verifies payment."
           if settings.mode == "x402-exact" else "")
    ))

    def failure() -> dict[str, Any]:
        return {"ok": False, "error": {"code": "METROPOLIS_UNAVAILABLE",
                "message": "retain the operation ID and recover by observation", "retry_without_new_payment": True}}

    if os.environ.get("EREBUS_WALLET_CONFIG") or os.environ.get("EREBUS_WALLET_POLICY"):
        wallet_config, wallet_data = _operator_file(os.environ.get("EREBUS_WALLET_CONFIG"))
        _, wallet_policy = _operator_file(os.environ.get("EREBUS_WALLET_POLICY"))
        required_policy = {"version", "maximum_amount", "allowed_methods", "withdrawal_recipients"}
        if wallet_data.get("state_root") != settings.state_root or not required_policy <= set(wallet_policy) <= required_policy | {"replacement_fees"} or type(wallet_policy.get("version")) is not int or wallet_policy["version"] != 1:
            raise ConfigError("wallet must share this operator's state root and explicit versioned action policy")
        methods, recipients = wallet_policy.get("allowed_methods"), wallet_policy.get("withdrawal_recipients")
        if not isinstance(methods, list) or any(not isinstance(method, str) for method in methods) or len(methods) != len(set(methods)) or not isinstance(recipients, list) or any(not isinstance(recipient, str) for recipient in recipients):
            raise ConfigError("wallet policy requires explicit unique action and recipient lists")
        try:
            wallet = MetropolisWallet(config_file=wallet_config,
                policy=WalletPolicy(wallet_policy["maximum_amount"], frozenset(methods), frozenset(recipients),
                    tuple(wallet_policy["replacement_fees"]) if isinstance(wallet_policy.get("replacement_fees"), list) else None),
                binary=os.environ.get("EREBUS_WALLET_CLI"), timeout=settings.timeout)
            wallet.version()
        except (MetropolisError, TypeError):
            raise ConfigError("invalid wallet action policy or missing native wallet command") from None

        async def wallet_action(method: str, **arguments: Any) -> dict[str, Any]:
            try:
                result = await asyncio.to_thread(wallet.execute, method, **arguments)
                return {"ok": result["status"] == "ok", "result": result}
            except MetropolisError:
                return {"ok": False, "error": {"code": "WALLET_UNAVAILABLE",
                    "message": "retain wallet operation IDs and sync; do not create another funding action",
                    "retry_without_new_payment": True}}

        @server.tool()
        async def wallet_notes() -> dict[str, Any]:
            """List this configured wallet's public note status without opening keys in Python."""
            return await wallet_action("notes")

        @server.tool()
        async def sync_wallet() -> dict[str, Any]:
            """Resume paired finalized history; never sends a transaction."""
            return await wallet_action("sync")

        if "status" in methods:
            @server.tool()
            async def wallet_status() -> dict[str, Any]:
                """Read paired finalized balances, pool allowance and checkpoint lag; no spend or sync."""
                return await wallet_action("status")

        if "approve" in methods:
            @server.tool()
            async def approve_wallet(amount: str, operation_ref: str) -> dict[str, Any]:
                """Explicit bounded approval for this configured pool and asset only."""
                return await wallet_action("approve", amount=amount, operation_ref=operation_ref)

        if "wrap" in methods:
            @server.tool()
            async def wrap_wallet(amount: str, operation_ref: str) -> dict[str, Any]:
                """Explicit bounded wrapping; retain and reuse the same operation reference."""
                return await wallet_action("wrap", amount=amount, operation_ref=operation_ref)

        if "deposit" in methods:
            @server.tool()
            async def deposit_wallet(amount: str, operation_ref: str) -> dict[str, Any]:
                """Fund one shielded note under a durable deposit reference; never use a new ID to retry."""
                return await wallet_action("deposit", amount=amount, operation_ref=operation_ref)

        if "withdraw" in methods:
            @server.tool()
            async def withdraw_wallet(commitment: str, recipient: str) -> dict[str, Any]:
                """Withdraw one bounded note only to an operator-authorized public recipient."""
                return await wallet_action("withdraw", commitment=commitment, recipient=recipient)

        if "unwrap" in methods:
            @server.tool()
            async def unwrap_wallet(amount: str, operation_ref: str) -> dict[str, Any]:
                """Explicit bounded unwrap; payout address and wrapper come from fixed configuration."""
                return await wallet_action("unwrap", amount=amount, operation_ref=operation_ref)

        if "resume" in methods:
            @server.tool()
            async def resume_wallet(identity: str) -> dict[str, Any]:
                """Explicitly resend only the retained bytes for this wallet operation."""
                return await wallet_action("resume", identity=identity)

        if "replace" in methods:
            @server.tool()
            async def replace_wallet(identity: str) -> dict[str, Any]:
                """Explicit fee replacement preserving intent; fees are fixed by native operator policy."""
                return await wallet_action("replace", identity=identity)

    @server.tool()
    async def negotiate_deal(operation_ref: str, freeze_only: bool = False) -> dict[str, Any]:
        """Negotiate and authorize one fixed-policy deal; never submits payment."""
        try:
            return {"ok": True, "result": await asyncio.to_thread(seam.negotiate, operation_ref, freeze_only=freeze_only)}
        except MetropolisError:
            return failure()

    if settings.role == "buyer" and settings.mode != "x402-exact":
        async def payment(operation_ref: str, method: str) -> dict[str, Any]:
            try:
                response = await asyncio.to_thread(seam.payment, operation_ref, method=method)
                return {"ok": response["status"] in {"ok", "ready"}, "result": response}
            except MetropolisError:
                return failure()

        @server.tool()
        async def check_settlement_funding(operation_ref: str) -> dict[str, Any]:
            """Check funding for an authorized deal without signing or submitting."""
            return await payment(operation_ref, "funding")

        if settings.submission_mode == "sponsored":
            @server.tool()
            async def sponsor_shielded_deal(operation_ref: str) -> dict[str, Any]:
                """Prove locally and delegate to the pinned sponsor; never directly broadcasts."""
                return await payment(operation_ref, "settle")
        else:
            @server.tool()
            async def settle_deal(operation_ref: str) -> dict[str, Any]:
                """Prepare, prove if shielded, and submit at most one automatic broadcast attempt."""
                return await payment(operation_ref, "settle")

        @server.tool()
        async def recover_deal(operation_ref: str) -> dict[str, Any]:
            """Observe and reconcile this operation; cannot sign, prove, or submit a payment."""
            return await payment(operation_ref, "observe")

    if settings.role == "buyer":
        if os.environ.get("EREBUS_ACCESS_SERVICE_URL"):
            from erebus_mcp.access import AccessSettings, register_access_tools

            if settings.mode != "x402-exact" and os.environ.get("EREBUS_ACCESS_PAYMENT_RAIL", "observe") != "observe":
                raise ConfigError("x402 authorization cannot be combined with ordinary settlement tools; use the dedicated access server")

            # Negotiation writes buyer evidence only under <state_root>/agent, so that is the
            # evidence directory; a separately configured one could only disagree with it.
            evidence = Path(settings.state_root) / "agent"
            configured = os.environ.get("EREBUS_ACCESS_EVIDENCE_DIR")
            if configured and Path(configured).resolve() != evidence.resolve():
                raise ConfigError("Metropolis access evidence is this participant's <state_root>/agent")
            try:
                evidence.mkdir(mode=0o700, parents=True, exist_ok=True)
            except OSError:
                raise ConfigError("cannot create the participant evidence directory") from None
            os.environ["EREBUS_ACCESS_EVIDENCE_DIR"] = str(evidence)
            register_access_tools(server, AccessSettings.from_env())
    return server
