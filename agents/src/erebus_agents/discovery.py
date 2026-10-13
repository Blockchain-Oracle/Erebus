"""A discovery directory of counterparties: capability, asset, price band and endpoint.

The signed publication primitive is Rust's `ServiceDescriptor`
(`sdk/transport/src/descriptor.rs`): an authorization address, an X25519 transport key,
endpoints, chain and assets, signed by the address that will later authorize settlement. That
type carries no price and no capability, on purpose.

This directory is the Python-side index a buyer agent searches: it pairs a capability and a
price band with a seller and the endpoint to reach it, and it can carry the digest of the
seller's signed descriptor as the binding back to the verifiable publication. It does not
verify the descriptor's signature -- that happens in Rust against the pinned suite -- so treat
a record as a pointer, not as proof. `resolve` returns the endpoint only for a record the
caller has already decided to trust.
"""

from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from pathlib import Path

from erebus_mcp.interface import AgentId

_SERVICE_ID = re.compile(r"[a-z0-9][a-z0-9._-]*")
_SHA256_HEX = re.compile(r"[a-f0-9]{64}")


@dataclass(frozen=True)
class ServiceRecord:
    service_id: str
    seller: AgentId
    capability: str
    asset: str
    min_unit_price: int
    max_unit_price: int
    endpoint: str
    descriptor_digest: str | None = None

    def __post_init__(self) -> None:
        if not _SERVICE_ID.fullmatch(self.service_id):
            raise ValueError("service_id must be a lowercase, greppable identifier")
        for name in ("seller", "capability", "asset", "endpoint"):
            if not getattr(self, name):
                raise ValueError(f"{name} must not be empty")
        if self.min_unit_price <= 0 or self.max_unit_price <= 0 or self.min_unit_price > self.max_unit_price:
            raise ValueError("unit prices must be positive and min must not exceed max")
        if self.descriptor_digest is not None and not _SHA256_HEX.fullmatch(self.descriptor_digest):
            raise ValueError("descriptor_digest must be lowercase 64-hex when present")

    def to_dict(self) -> dict[str, object]:
        return {
            "service_id": self.service_id, "seller": self.seller, "capability": self.capability,
            "asset": self.asset, "min_unit_price": self.min_unit_price, "max_unit_price": self.max_unit_price,
            "endpoint": self.endpoint, "descriptor_digest": self.descriptor_digest,
        }

    @classmethod
    def from_dict(cls, value: dict[str, object]) -> "ServiceRecord":
        digest = value.get("descriptor_digest")
        return cls(
            service_id=str(value["service_id"]), seller=str(value["seller"]),
            capability=str(value["capability"]), asset=str(value["asset"]),
            min_unit_price=int(value["min_unit_price"]),  # type: ignore[arg-type]
            max_unit_price=int(value["max_unit_price"]),  # type: ignore[arg-type]
            endpoint=str(value["endpoint"]), descriptor_digest=None if digest is None else str(digest))


class DiscoveryDirectory:
    """A local index of service records, keyed by `service_id`."""

    def __init__(self, path: Path | None = None) -> None:
        self._path = path
        self._records: dict[str, ServiceRecord] = {}

    def publish(self, record: ServiceRecord) -> None:
        existing = self._records.get(record.service_id)
        if existing is not None and existing != record:
            raise ValueError(f"service_id {record.service_id!r} is already published with other terms")
        self._records[record.service_id] = record

    def retire(self, service_id: str) -> None:
        if service_id not in self._records:
            raise KeyError(service_id)
        del self._records[service_id]

    def records(self) -> tuple[ServiceRecord, ...]:
        return tuple(self._records[key] for key in sorted(self._records))

    def search(self, capability: str, *, asset: str | None = None, max_unit_price: int | None = None,
               require_descriptor: bool = False) -> tuple[ServiceRecord, ...]:
        """Cheapest-first records matching the filters; ties break on `service_id`."""
        found = [
            record for record in self._records.values()
            if record.capability == capability
            and (asset is None or record.asset == asset)
            and (max_unit_price is None or record.min_unit_price <= max_unit_price)
            and (not require_descriptor or record.descriptor_digest is not None)
        ]
        found.sort(key=lambda record: (record.min_unit_price, record.service_id))
        return tuple(found)

    def resolve(self, service_id: str) -> str:
        record = self._records.get(service_id)
        if record is None:
            raise KeyError(service_id)
        return record.endpoint

    def save(self, path: Path | None = None) -> Path:
        target = path or self._path
        if target is None:
            raise ValueError("no directory path configured")
        payload = json.dumps({"records": [record.to_dict() for record in self.records()]}, indent=2)
        tmp = target.with_suffix(target.suffix + ".tmp")
        tmp.write_text(payload)
        tmp.replace(target)
        if os.name == "posix":
            target.chmod(0o600)
        return target

    @classmethod
    def load(cls, path: Path) -> "DiscoveryDirectory":
        directory = cls(path)
        for entry in json.loads(path.read_text()).get("records", []):
            directory.publish(ServiceRecord.from_dict(entry))
        return directory
