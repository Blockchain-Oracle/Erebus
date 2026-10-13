"""Path-only buyer access. Rust signs locally, verifies resource bytes, and keeps the cursor."""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from pathlib import Path
from typing import Any

_U64_MAX = 2**64 - 1
_HEX32 = re.compile(r"[a-f0-9]{64}")
_FAILURE_MESSAGES = {
    "configuration": "configure a trusted HTTPS metered endpoint and private local cache",
    "agreement": "agreement does not support metered access",
    "authentication": "configure the existing buyer agreement key for access",
    "pending": "access is pending; retry the same call without another payment",
    "unavailable": "metered service unavailable; retain state and retry the same call without another payment",
    "response_invalid": "metered response failed verification; do not pay again",
    "storage": "local metered cache unavailable; retain state and retry without another payment",
    "seller_ledger_behind": "seller ledger is behind the local cursor; stop and report it, do not pay again",
    "allowance_exhausted": "prepaid allowance is exhausted",
    "call_index_ahead": "call index is ahead of the next unconsumed call",
}


class AccessError(RuntimeError):
    """Retrieval failed. Never authorize another payment from this error.

    `code` is the Rust command's stable error code when it reported one, so a caller can tell
    lost seller state from exhaustion or a transient outage without parsing the message.
    """

    def __init__(self, message: str, code: str | None = None) -> None:
        super().__init__(message)
        self.code = code

    def public_failure(self) -> dict[str, str]:
        """Fixed model-visible details; never forward an arbitrary exception message/code."""
        code = self.code if isinstance(self.code, str) and self.code in _FAILURE_MESSAGES else None
        result = {"message": _FAILURE_MESSAGES.get(code, "retrieval failed; retain state and retry without another payment")}
        if code is not None:
            result["error_code"] = code
        return result


class AccessSeam:
    """Forward local paths; return content metadata, not private keys or resource contents."""

    def __init__(self, binary: str | Path | None = None, timeout: int = 120) -> None:
        resolved = str(binary) if binary is not None else shutil.which("erebus-access")
        if not resolved or type(timeout) is not int or not 1 <= timeout <= 600:
            raise AccessError("configure an access binary and bounded timeout")
        self._binary = resolved
        self._timeout = timeout

    def _run(self, request: dict[str, Any] | None) -> tuple[int, dict[str, Any]]:
        try:
            output = subprocess.run(
                [self._binary] + (["--version"] if request is None else []),
                input=None if request is None else json.dumps(request),
                capture_output=True, text=True, timeout=self._timeout, check=False,
            )
        except (OSError, subprocess.TimeoutExpired, UnicodeError):
            raise AccessError("retrieval unavailable; retain state and retry without another payment") from None
        if output.returncode not in {0, 1, 2} or output.stderr or len(output.stdout) > 16_384:
            raise AccessError("retrieval failed; retain state and retry without another payment")
        try:
            response = json.loads(output.stdout)
        except (ValueError, TypeError):
            raise AccessError("invalid access response") from None
        if not isinstance(response, dict):
            raise AccessError("invalid access response")
        if output.returncode == 1:
            self._failure(response)
        return output.returncode, response

    @staticmethod
    def _failure(response: dict[str, Any]) -> None:
        """Raise one validated hard failure, preserving the command's stable error code."""
        code = response.get("error_code")
        if (
            not set(response).issubset({"status", "error", "error_code", "retry_without_payment"})
            or response.get("status") != "error"
            or not isinstance(response.get("error"), str) or not response["error"]
            or response.get("retry_without_payment") is not True
            or (code is not None and (not isinstance(code, str) or code not in _FAILURE_MESSAGES))
        ):
            raise AccessError("invalid access failure response")
        message = _FAILURE_MESSAGES.get(code, "retrieval failed; retain state and retry without another payment")
        raise AccessError(message, code)

    def _common(self, evidence_file: str, buyer_key_file: str, service_url: str,
                service_id: str, cache_root: str, allow_loopback_http: bool) -> dict[str, Any]:
        if (
            any(not isinstance(path, str) or not path or len(path) > 4096 for path in (evidence_file, buyer_key_file, cache_root, service_url))
            or not isinstance(service_id, str) or not _HEX32.fullmatch(service_id)
            or service_id == "0" * 64 or type(allow_loopback_http) is not bool
        ):
            raise AccessError("invalid access request")
        return {"evidence_file": evidence_file, "buyer_key_file": buyer_key_file,
                "service_url": service_url, "service_id": service_id, "cache_root": cache_root,
                "allow_loopback_http": allow_loopback_http}

    @staticmethod
    def _pending(response: dict[str, Any]) -> None:
        expected = {"status": "pending", "retry_without_payment": True, "payment_verified": False,
                    "resource_verified": False, "delivery_verified": False}
        if response != expected or any(type(response[field]) is not bool for field in expected if field != "status"):
            raise AccessError("invalid pending metered response")

    @staticmethod
    def _hex32(value: Any) -> bool:
        return isinstance(value, str) and _HEX32.fullmatch(value) is not None

    @staticmethod
    def _count(value: Any) -> bool:
        return type(value) is int and 0 <= value <= _U64_MAX

    def version(self) -> dict[str, Any]:
        """Check the binary protocol without reading keys or accessing the service."""
        code, response = self._run(None)
        if code != 0 or response != {"status": "ok", "protocol": 2, "methods": ["retrieve", "metered_call", "metered_status"]} or type(response["protocol"]) is not int:
            raise AccessError("access binary protocol mismatch")
        return response

    def retrieve(self, *, evidence_file: str, buyer_key_file: str, service_url: str,
                 service_id: str, cache_root: str, allow_loopback_http: bool = False,
                 x402_exact: bool = False) -> dict[str, Any]:
        """Authenticate locally and retrieve once. Pending is not verified payment."""
        if type(x402_exact) is not bool:
            raise AccessError("invalid access request")
        request = self._common(evidence_file, buyer_key_file, service_url, service_id, cache_root, allow_loopback_http)
        request["method"] = "retrieve"
        if x402_exact:
            request["x402_exact"] = True
        code, response = self._run(request)
        if code == 2:
            status = response.get("status")
            if not isinstance(status, str) or status not in {"pending", "paid_but_undelivered"}:
                raise AccessError("invalid recoverable access response")
            expected = {"status": status, "retry_without_payment": True, "payment_verified": False,
                        "resource_verified": False, "delivery_verified": False}
            if status == "paid_but_undelivered":
                expected["seller_reported_payment_finalized"] = True
            if response != expected or any(type(response[field]) is not bool for field in expected if field != "status"):
                raise AccessError("invalid pending access response")
            return response
        if set(response) != {"status", "result"} or response["status"] != "retrieved" or not isinstance(response["result"], dict):
            raise AccessError("invalid retrieval response")
        result = response["result"]
        fields = {"deal_commitment", "issuance_id", "resource_sha256", "resource_bytes", "resource_file", "cached",
                  "seller_reported_payment_finalized", "payment_verified", "resource_verified", "delivery_verified"}
        if set(result) != fields:
            raise AccessError("unexpected retrieval response fields")
        if (
            any(not self._hex32(result[name]) for name in ("deal_commitment", "issuance_id", "resource_sha256"))
            or type(result["resource_bytes"]) is not int or not 0 <= result["resource_bytes"] <= 1024 * 1024
            or not isinstance(result["resource_file"], str) or not 1 <= len(result["resource_file"]) <= 4096
            or Path(result["resource_file"]) != Path(cache_root) / f"{result['issuance_id']}.0.tx"
            or any(type(result[name]) is not bool for name in ("cached", "seller_reported_payment_finalized", "payment_verified", "resource_verified", "delivery_verified"))
            or not result["seller_reported_payment_finalized"] or result["payment_verified"]
            or not result["resource_verified"] or result["delivery_verified"]
        ):
            raise AccessError("invalid retrieval metadata or verification claims")
        return response

    def metered_call(self, *, evidence_file: str, buyer_key_file: str, service_url: str,
                     service_id: str, cache_root: str, allow_loopback_http: bool = False,
                     call_index: int | None = None) -> dict[str, Any]:
        """Consume one prepaid call, finish the call in flight, or return an earlier one.

        Rust keeps the durable cursor and re-signs the same in-flight index after a timeout,
        pending payment, or lost reply. Never authorize another payment from a failed call.
        """
        request = self._common(evidence_file, buyer_key_file, service_url, service_id, cache_root, allow_loopback_http)
        if call_index is not None:
            if not self._count(call_index):
                raise AccessError("invalid access request")
            request["call_index"] = call_index
        request["method"] = "metered_call"
        code, response = self._run(request)
        if code == 2:
            self._pending(response)
            return response
        if set(response) != {"status", "result"} or response["status"] != "issued" or not isinstance(response["result"], dict):
            raise AccessError("invalid metered call response")
        result = response["result"]
        fields = {"deal_commitment", "entitlement_id", "call_index", "quantity", "next_call_index", "remaining",
                  "response_sha256", "response_bytes", "response_file", "repeated", "cached", "late",
                  "seller_reported_payment_finalized", "response_hash_consistent", "resource_verified",
                  "payment_verified", "delivery_verified"}
        if set(result) != fields:
            raise AccessError("unexpected metered call response fields")
        if (
            any(not self._hex32(result[name]) for name in ("deal_commitment", "entitlement_id", "response_sha256"))
            or any(not self._count(result[name]) for name in ("call_index", "quantity", "next_call_index", "remaining", "response_bytes"))
            or not isinstance(result["response_file"], str) or not 1 <= len(result["response_file"]) <= 4096
            or Path(result["response_file"]).parent != Path(cache_root) / "metered" / "calls"
            or re.fullmatch(r"[a-f0-9]{64}\.0\.tx", Path(result["response_file"]).name) is None
            or any(type(result[name]) is not bool for name in ("repeated", "cached", "late", "seller_reported_payment_finalized", "response_hash_consistent", "resource_verified", "payment_verified", "delivery_verified"))
            or not result["seller_reported_payment_finalized"] or not result["response_hash_consistent"]
            or result["resource_verified"] or result["payment_verified"] or result["delivery_verified"]
        ):
            raise AccessError("invalid metered call metadata or verification claims")
        return response

    def metered_status(self, *, evidence_file: str, buyer_key_file: str, service_url: str,
                       service_id: str, cache_root: str, allow_loopback_http: bool = False) -> dict[str, Any]:
        """Report the seller's allowance and reconcile the local cursor. Consumes nothing."""
        request = self._common(evidence_file, buyer_key_file, service_url, service_id, cache_root, allow_loopback_http)
        request["method"] = "metered_status"
        code, response = self._run(request)
        if code == 2:
            self._pending(response)
            return response
        if set(response) != {"status", "result"} or response["status"] != "allowance" or not isinstance(response["result"], dict):
            raise AccessError("invalid metered status response")
        result = response["result"]
        fields = {"deal_commitment", "entitlement_id", "quantity", "seller_next_call_index", "remaining",
                  "next_call_index", "in_flight", "exhausted", "recovered_calls",
                  "seller_reported_payment_finalized", "payment_verified", "delivery_verified"}
        if set(result) != fields:
            raise AccessError("unexpected metered status response fields")
        if (
            any(not self._hex32(result[name]) for name in ("deal_commitment", "entitlement_id"))
            or any(not self._count(result[name]) for name in ("quantity", "seller_next_call_index", "remaining", "next_call_index", "recovered_calls"))
            or (result["in_flight"] is not None and not self._count(result["in_flight"]))
            or any(type(result[name]) is not bool for name in ("exhausted", "seller_reported_payment_finalized", "payment_verified", "delivery_verified"))
            or not result["seller_reported_payment_finalized"]
            or result["payment_verified"] or result["delivery_verified"]
        ):
            raise AccessError("invalid metered status metadata or verification claims")
        return response
