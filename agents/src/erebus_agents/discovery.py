"""A discovery directory of counterparties, indexed from their real signed service descriptors.

The signed publication primitive is Rust's `ServiceDescriptor` (`sdk/transport/src/descriptor.rs`),
written by `erebus-negotiate prepare_operator` as `<role>.descriptor.json`: the seller's
authorization address, transport key, endpoints, chain, assets, settlement modes and expiry,
signed by the address that will later authorize settlement. That type carries no price and no
capability, on purpose.

This directory is the index a buyer agent searches. Each record pairs a capability and an
advertised unit-price band (unsigned, the seller's claim) with one descriptor file. Everything
else in the record -- address, endpoints, chain, assets, modes, expiry -- is read from that file,
never typed in, and the file's bytes are pinned by SHA-256 so a later swap is detected. The
directory does not verify the descriptor's signature: Rust does, against the pinned suite, when
the buyer operator configures that file as its `peer_descriptor_file`. Treat a record as a pointer
to a descriptor, not as proof.
"""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path

from erebus_agents.marketplace import write_private_json

_SERVICE_ID = re.compile(r"[a-z0-9][a-z0-9._-]*")
_ADDRESS = re.compile(r"[a-f0-9]{40}")
MODE_PUBLIC_BOUND = 0b01
MODE_SHIELDED = 0b10
_MODES = {"public-bound": MODE_PUBLIC_BOUND, "shielded": MODE_SHIELDED}
_MAX_DESCRIPTOR_BYTES = 64 * 1024


def _read_descriptor(path: Path) -> tuple[bytes, dict[str, object]]:
    if not path.is_absolute():
        raise ValueError("descriptor_file must be an absolute path")
    data = path.read_bytes()
    if not 1 <= len(data) <= _MAX_DESCRIPTOR_BYTES:
        raise ValueError("descriptor file is empty or too large")
    document = json.loads(data)
    if not isinstance(document, dict):
        raise ValueError("descriptor file is not a descriptor document")
    return data, document


@dataclass(frozen=True)
class ServiceRecord:
    service_id: str
    capability: str
    min_unit_price: int
    max_unit_price: int
    descriptor_file: str
    descriptor_sha256: str
    seller_address: str
    endpoints: tuple[str, ...]
    chain_namespace: str
    assets: tuple[str, ...]
    settlement_modes: int
    expires: int

    def __post_init__(self) -> None:
        if not _SERVICE_ID.fullmatch(self.service_id):
            raise ValueError("service_id must be a lowercase, greppable identifier")
        if not self.capability:
            raise ValueError("capability must not be empty")
        if self.min_unit_price <= 0 or self.max_unit_price <= 0 or self.min_unit_price > self.max_unit_price:
            raise ValueError("unit prices must be positive and min must not exceed max")
        if not _ADDRESS.fullmatch(self.seller_address):
            raise ValueError("descriptor seller_address must be 20 lowercase hex bytes")
        if not self.endpoints or not all(isinstance(endpoint, str) and endpoint for endpoint in self.endpoints):
            raise ValueError("descriptor must list at least one endpoint")
        if not self.chain_namespace or not self.assets:
            raise ValueError("descriptor must name a chain and at least one asset")
        if not self.settlement_modes or self.settlement_modes & ~(MODE_PUBLIC_BOUND | MODE_SHIELDED):
            raise ValueError("descriptor settlement modes are unknown")

    @classmethod
    def from_descriptor(cls, service_id: str, capability: str, min_unit_price: int, max_unit_price: int,
                        descriptor_file: Path) -> "ServiceRecord":
        """Index one signed descriptor file under a capability and an advertised price band."""
        data, document = _read_descriptor(descriptor_file)
        try:
            return cls(
                service_id=service_id, capability=capability,
                min_unit_price=min_unit_price, max_unit_price=max_unit_price,
                descriptor_file=str(descriptor_file), descriptor_sha256=hashlib.sha256(data).hexdigest(),
                seller_address=str(document["seller_address"]),
                endpoints=tuple(document["endpoints"]),  # type: ignore[arg-type]
                chain_namespace=str(document["chain_namespace"]),
                assets=tuple(str(asset) for asset in document["assets"]),  # type: ignore[union-attr]
                settlement_modes=int(document["settlement_modes"]),  # type: ignore[call-overload]
                expires=int(document["expires"]),  # type: ignore[call-overload]
            )
        except (KeyError, TypeError):
            raise ValueError("descriptor file is missing descriptor fields") from None

    def is_current(self, now: int) -> bool:
        return now < self.expires

    def supports_mode(self, mode: str) -> bool:
        if mode not in _MODES:
            raise ValueError("mode must be public-bound or shielded")
        return bool(self.settlement_modes & _MODES[mode])

    def unchanged(self) -> bool:
        """True while the descriptor file still holds exactly the bytes this record indexed."""
        try:
            data, _ = _read_descriptor(Path(self.descriptor_file))
        except (OSError, ValueError):
            return False
        return hashlib.sha256(data).hexdigest() == self.descriptor_sha256

    def to_dict(self) -> dict[str, object]:
        return {
            "service_id": self.service_id, "capability": self.capability,
            "min_unit_price": self.min_unit_price, "max_unit_price": self.max_unit_price,
            "descriptor_file": self.descriptor_file, "descriptor_sha256": self.descriptor_sha256,
            "seller_address": self.seller_address, "endpoints": list(self.endpoints),
            "chain_namespace": self.chain_namespace, "assets": list(self.assets),
            "settlement_modes": self.settlement_modes, "expires": self.expires,
        }

    @classmethod
    def from_dict(cls, value: dict[str, object]) -> "ServiceRecord":
        """Re-index from the descriptor file and refuse it if its bytes changed since publishing."""
        record = cls.from_descriptor(
            str(value["service_id"]), str(value["capability"]),
            int(value["min_unit_price"]), int(value["max_unit_price"]),  # type: ignore[call-overload]
            Path(str(value["descriptor_file"])))
        if record.descriptor_sha256 != value.get("descriptor_sha256"):
            raise ValueError(f"descriptor for {record.service_id!r} changed since it was published")
        return record


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

    def search(self, capability: str, *, now: int, asset: str | None = None, max_unit_price: int | None = None,
               mode: str | None = None) -> tuple[ServiceRecord, ...]:
        """Cheapest-first current records matching the filters; ties break on `service_id`."""
        found = [
            record for record in self._records.values()
            if record.capability == capability and record.is_current(now)
            and (asset is None or asset in record.assets)
            and (max_unit_price is None or record.min_unit_price <= max_unit_price)
            and (mode is None or record.supports_mode(mode))
        ]
        found.sort(key=lambda record: (record.min_unit_price, record.service_id))
        return tuple(found)

    def resolve(self, service_id: str, *, now: int) -> ServiceRecord:
        """The record a buyer operator can configure from, refused if expired or swapped."""
        record = self._records.get(service_id)
        if record is None:
            raise KeyError(service_id)
        if not record.is_current(now):
            raise ValueError(f"descriptor for {service_id!r} has expired")
        if not record.unchanged():
            raise ValueError(f"descriptor for {service_id!r} changed since it was published")
        return record

    def save(self, path: Path | None = None) -> Path:
        target = path or self._path
        if target is None:
            raise ValueError("no directory path configured")
        write_private_json(target, {"records": [record.to_dict() for record in self.records()]})
        return target

    @classmethod
    def load(cls, path: Path) -> "DiscoveryDirectory":
        directory = cls(path)
        for entry in json.loads(path.read_text()).get("records", []):
            directory.publish(ServiceRecord.from_dict(entry))
        return directory
