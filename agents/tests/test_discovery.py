"""Discovery directory: records come from descriptor files, are pinned, and refuse swaps and expiry."""

import json
import subprocess
from pathlib import Path

import pytest

from erebus_agents.discovery import DiscoveryDirectory, ServiceRecord

ROOT = Path(__file__).resolve().parents[2]
NEGOTIATE = ROOT / "sdk/shielded/target/release/erebus-negotiate"
NOW = 1_800_000_000
ASSET = "eip155:10143/erc20:0x5fc8d32690cc91d4c39d9d3abcbd16989f875707"
CAPABILITY = "summarize-documents"


def descriptor(tmp_path, name, *, seller="11" * 20, modes=2, expires=NOW + 3600, assets=(ASSET,)):
    """A descriptor-shaped document with the fields the index reads (the Rust JSON field names)."""
    path = tmp_path / f"{name}.descriptor.json"
    path.write_text(json.dumps({"protocol_version": 1, "seller_address": seller, "endpoints": [f"tcp://{name}:7443"],
                                "chain_namespace": "eip155:10143", "assets": list(assets), "suites": [2],
                                "settlement_modes": modes, "guarantees": 7, "issued_at": NOW - 60,
                                "expires": expires}))
    return path


def record(tmp_path, service_id, minimum, maximum, **options):
    return ServiceRecord.from_descriptor(service_id, CAPABILITY, minimum, maximum,
                                         descriptor(tmp_path, service_id, **options))


def test_record_fields_come_from_the_descriptor_and_are_validated(tmp_path):
    entry = record(tmp_path, "svc-atlas", 3, 7)
    assert entry.seller_address == "11" * 20 and entry.endpoints == ("tcp://svc-atlas:7443",)
    assert entry.assets == (ASSET,) and entry.supports_mode("shielded") and not entry.supports_mode("public-bound")
    with pytest.raises(ValueError):
        record(tmp_path, "svc-bad", 5, 4)
    with pytest.raises(ValueError, match="seller_address"):
        record(tmp_path, "svc-bad", 1, 2, seller="0xABC")
    with pytest.raises(ValueError, match="modes"):
        record(tmp_path, "svc-bad", 1, 2, modes=4)
    with pytest.raises(ValueError, match="absolute"):
        ServiceRecord.from_descriptor("svc-rel", CAPABILITY, 1, 2, Path("relative.descriptor.json"))
    missing = tmp_path / "missing.descriptor.json"
    missing.write_text(json.dumps({"seller_address": "11" * 20}))
    with pytest.raises(ValueError, match="missing descriptor fields"):
        ServiceRecord.from_descriptor("svc-missing", CAPABILITY, 1, 2, missing)


def test_search_filters_on_capability_asset_price_mode_and_expiry(tmp_path):
    directory = DiscoveryDirectory()
    directory.publish(record(tmp_path, "svc-atlas", 3, 7))
    directory.publish(record(tmp_path, "svc-aegean", 4, 9, modes=3))
    directory.publish(record(tmp_path, "svc-boreas", 2, 5, assets=("eip155:10143/erc20:0xother",)))
    directory.publish(record(tmp_path, "svc-stale", 1, 2, expires=NOW - 1))
    ids = lambda found: [entry.service_id for entry in found]
    assert ids(directory.search(CAPABILITY, now=NOW, asset=ASSET)) == ["svc-atlas", "svc-aegean"]
    assert ids(directory.search(CAPABILITY, now=NOW, asset=ASSET, max_unit_price=3)) == ["svc-atlas"]
    assert ids(directory.search(CAPABILITY, now=NOW, mode="public-bound")) == ["svc-aegean"]
    assert "svc-stale" not in ids(directory.search(CAPABILITY, now=NOW))


def test_resolve_refuses_an_expired_or_swapped_descriptor(tmp_path):
    directory = DiscoveryDirectory()
    atlas = record(tmp_path, "svc-atlas", 3, 7)
    directory.publish(atlas)
    assert directory.resolve("svc-atlas", now=NOW).descriptor_file == atlas.descriptor_file
    with pytest.raises(ValueError, match="expired"):
        directory.resolve("svc-atlas", now=atlas.expires)
    descriptor(tmp_path, "svc-atlas", seller="22" * 20)
    with pytest.raises(ValueError, match="changed"):
        directory.resolve("svc-atlas", now=NOW)
    with pytest.raises(KeyError):
        directory.resolve("missing", now=NOW)


def test_publish_conflict_retire_and_owner_only_round_trip(tmp_path):
    directory = DiscoveryDirectory()
    atlas = record(tmp_path, "svc-atlas", 3, 7)
    directory.publish(atlas)
    directory.publish(atlas)
    with pytest.raises(ValueError, match="already published"):
        directory.publish(ServiceRecord.from_descriptor("svc-atlas", CAPABILITY, 4, 7, Path(atlas.descriptor_file)))
    path = directory.save(tmp_path / "directory.json")
    assert (path.stat().st_mode & 0o777) == 0o600
    assert [entry.to_dict() for entry in DiscoveryDirectory.load(path).records()] == [atlas.to_dict()]
    descriptor(tmp_path, "svc-atlas", seller="33" * 20)
    with pytest.raises(ValueError, match="changed"):
        DiscoveryDirectory.load(path)
    directory.retire("svc-atlas")
    with pytest.raises(KeyError):
        directory.retire("svc-atlas")


@pytest.mark.skipif(not NEGOTIATE.exists(), reason="build the negotiation binary")
def test_indexes_a_descriptor_signed_by_the_real_negotiation_binary(tmp_path):
    request = {"method": "prepare_operator", "suite_id": 2, "directory": str(tmp_path / "seller"), "role": "seller",
               "endpoint": "127.0.0.1:7443", "namespace": "eip155:10143", "assets": [ASSET],
               "descriptor_lifetime_seconds": 3600}
    completed = subprocess.run([str(NEGOTIATE)], input=json.dumps(request), capture_output=True, text=True, check=True)
    prepared = json.loads(completed.stdout)
    entry = ServiceRecord.from_descriptor("svc-real", CAPABILITY, 3, 7, Path(prepared["descriptor_file"]))
    assert entry.seller_address == prepared["discovery_address"].removeprefix("0x")
    assert entry.endpoints == ("tcp://127.0.0.1:7443",) and entry.assets == (ASSET,)
    assert entry.expires == prepared["descriptor_expires_at"] and entry.unchanged()
