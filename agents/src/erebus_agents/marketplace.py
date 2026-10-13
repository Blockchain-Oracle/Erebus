"""A capability marketplace over the negotiation and settlement loop.

A seller publishes a capability with a unit-price band and an asset. A buyer searches the
registry for a capability, picks the cheapest listing inside its ceiling, negotiates, and
pays once. Nothing here is protocol logic: the registry is ordinary I/O and the delivery
record is ordinary policy.

Delivery is an off-chain acknowledgement, never settlement evidence. The proof says nothing
about whether a resource arrived, so a `DeliveryRecord` records what an operator observed and
an auditor may read it, but `DELIVERED` is not proof of payment and payment is not proof of
delivery. The two are kept in separate states on purpose (see `docs/usecases.md`, "One-off
purchase of an off-chain service").
"""

from __future__ import annotations

import json
import os
import re
import tempfile
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from erebus_mcp.interface import AgentId

_SERVICE_ID = re.compile(r"[a-z0-9][a-z0-9._-]*")
_SHA256_HEX = re.compile(r"[a-f0-9]{64}")


def write_private_json(path: Path, value: dict[str, object]) -> None:
    """Atomically replace `path` with `value`, owner-only from the first byte written."""
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.chmod(temporary, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            descriptor = -1
            json.dump(value, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


@dataclass(frozen=True)
class ServiceListing:
    """One seller's published capability: what it does, in what unit, and between which prices."""

    service_id: str
    seller: AgentId
    capability: str
    asset: str
    unit: str
    min_unit_price: int
    max_unit_price: int

    def __post_init__(self) -> None:
        if not _SERVICE_ID.fullmatch(self.service_id):
            raise ValueError("service_id must be a lowercase, greppable identifier")
        for field in ("seller", "capability", "asset", "unit"):
            if not getattr(self, field):
                raise ValueError(f"{field} must not be empty")
        if self.min_unit_price <= 0 or self.max_unit_price <= 0:
            raise ValueError("unit prices must be positive")
        if self.min_unit_price > self.max_unit_price:
            raise ValueError("min_unit_price must not exceed max_unit_price")

    def to_dict(self) -> dict[str, object]:
        return {
            "service_id": self.service_id,
            "seller": self.seller,
            "capability": self.capability,
            "asset": self.asset,
            "unit": self.unit,
            "min_unit_price": self.min_unit_price,
            "max_unit_price": self.max_unit_price,
        }

    @classmethod
    def from_dict(cls, value: dict[str, object]) -> "ServiceListing":
        return cls(
            service_id=str(value["service_id"]),
            seller=str(value["seller"]),
            capability=str(value["capability"]),
            asset=str(value["asset"]),
            unit=str(value["unit"]),
            min_unit_price=int(value["min_unit_price"]),  # type: ignore[arg-type]
            max_unit_price=int(value["max_unit_price"]),  # type: ignore[arg-type]
        )


class ServiceRegistry:
    """A local, file-backed catalog of listings, keyed by `service_id`.

    Publishing the same listing twice is idempotent; publishing a different listing under an
    existing `service_id` is a conflict, because a buyer that cached the old terms must not be
    silently redirected to a new seller.
    """

    def __init__(self, path: Path | None = None) -> None:
        self._path = path
        self._listings: dict[str, ServiceListing] = {}

    def publish(self, listing: ServiceListing) -> None:
        existing = self._listings.get(listing.service_id)
        if existing is not None and existing != listing:
            raise ValueError(f"service_id {listing.service_id!r} is already published with other terms")
        self._listings[listing.service_id] = listing

    def retire(self, service_id: str) -> None:
        if service_id not in self._listings:
            raise KeyError(service_id)
        del self._listings[service_id]

    def listings(self) -> tuple[ServiceListing, ...]:
        return tuple(self._listings[key] for key in sorted(self._listings))

    def search(self, capability: str, *, max_unit_price: int | None = None) -> tuple[ServiceListing, ...]:
        """Listings for one capability, cheapest first, optionally capped at a unit price."""
        found = [
            listing
            for listing in self._listings.values()
            if listing.capability == capability
            and (max_unit_price is None or listing.min_unit_price <= max_unit_price)
        ]
        found.sort(key=lambda listing: (listing.min_unit_price, listing.service_id))
        return tuple(found)

    def best(self, capability: str, *, max_unit_price: int | None = None) -> ServiceListing | None:
        found = self.search(capability, max_unit_price=max_unit_price)
        return found[0] if found else None

    def save(self, path: Path | None = None) -> Path:
        target = path or self._path
        if target is None:
            raise ValueError("no registry path configured")
        write_private_json(target, {"listings": [listing.to_dict() for listing in self.listings()]})
        return target

    @classmethod
    def load(cls, path: Path) -> "ServiceRegistry":
        registry = cls(path)
        for entry in json.loads(path.read_text()).get("listings", []):
            registry.publish(ServiceListing.from_dict(entry))
        return registry


class DeliveryStatus(str, Enum):
    PENDING = "pending"
    DELIVERED = "delivered"
    REFUNDED = "refunded"
    DISPUTED = "disputed"


@dataclass(frozen=True)
class DeliveryRecord:
    """The off-chain half of a deal, tracked separately from settlement.

    `artifact_sha256` is present only once delivered. It is a local observation: it binds a
    byte string to this record, not to the chain and not to the payment.
    """

    deal_ref: str
    service_id: str
    status: DeliveryStatus
    artifact_sha256: str | None = None

    def __post_init__(self) -> None:
        if not self.deal_ref or not self.service_id:
            raise ValueError("deal_ref and service_id must not be empty")
        if self.status is DeliveryStatus.DELIVERED:
            if self.artifact_sha256 is None or not _SHA256_HEX.fullmatch(self.artifact_sha256):
                raise ValueError("a delivered record needs a lowercase 64-hex artifact_sha256")
        elif self.artifact_sha256 is not None and not _SHA256_HEX.fullmatch(self.artifact_sha256):
            raise ValueError("artifact_sha256 must be lowercase 64-hex when present")
        elif self.artifact_sha256 is not None and self.status is DeliveryStatus.PENDING:
            raise ValueError("a pending record does not carry an artifact hash")


def mark_delivered(record: DeliveryRecord, artifact_sha256: str) -> DeliveryRecord:
    if record.status is not DeliveryStatus.PENDING:
        raise ValueError(f"cannot deliver a {record.status.value} record")
    return DeliveryRecord(record.deal_ref, record.service_id, DeliveryStatus.DELIVERED, artifact_sha256.lower())


def mark_refunded(record: DeliveryRecord) -> DeliveryRecord:
    if record.status not in (DeliveryStatus.PENDING, DeliveryStatus.DISPUTED):
        raise ValueError(f"cannot refund a {record.status.value} record")
    return DeliveryRecord(record.deal_ref, record.service_id, DeliveryStatus.REFUNDED)


def mark_disputed(record: DeliveryRecord) -> DeliveryRecord:
    if record.status not in (DeliveryStatus.PENDING, DeliveryStatus.DELIVERED):
        raise ValueError(f"cannot dispute a {record.status.value} record")
    return DeliveryRecord(record.deal_ref, record.service_id, DeliveryStatus.DISPUTED, record.artifact_sha256)
