"""Discovery directory: record validation, filtered search, resolve, and persistence."""

import pytest

from erebus_agents.discovery import DiscoveryDirectory, ServiceRecord
from erebus_agents.discovery_demo import CAPABILITY, DIGEST, rehearse, seed

ATLAS = ServiceRecord("svc-atlas", "0xatlas", CAPABILITY, "0xusdc", 3, 7, "tcp://atlas:1", DIGEST)


def test_record_validation():
    with pytest.raises(ValueError):
        ServiceRecord("Svc", "0xa", "c", "0xu", 1, 2, "tcp://x:1")
    with pytest.raises(ValueError):
        ServiceRecord("svc", "0xa", "c", "0xu", 5, 4, "tcp://x:1")
    with pytest.raises(ValueError):
        ServiceRecord("svc", "0xa", "c", "0xu", 1, 2, "tcp://x:1", "not-hex")


def test_publish_conflict_and_retire():
    directory = DiscoveryDirectory()
    directory.publish(ATLAS)
    directory.publish(ATLAS)
    with pytest.raises(ValueError, match="already published"):
        directory.publish(ServiceRecord("svc-atlas", "0xother", CAPABILITY, "0xusdc", 3, 7, "tcp://y:1"))
    directory.retire("svc-atlas")
    with pytest.raises(KeyError):
        directory.retire("svc-atlas")


def test_search_filters_and_resolve():
    directory = seed()
    assert [r.service_id for r in directory.search(CAPABILITY, asset="0xusdc")] == ["svc-atlas", "svc-aegean"]
    assert [r.service_id for r in directory.search(CAPABILITY, asset="0xusdc", max_unit_price=3)] == ["svc-atlas"]
    assert [r.service_id for r in directory.search(CAPABILITY, require_descriptor=True)] == ["svc-atlas", "svc-boreas"]
    assert directory.resolve("svc-atlas") == "tcp://atlas.example:7443"
    with pytest.raises(KeyError):
        directory.resolve("missing")


def test_round_trip_preserves_records(tmp_path):
    directory = seed()
    path = directory.save(tmp_path / "directory.json")
    assert (path.stat().st_mode & 0o777) == 0o600
    loaded = DiscoveryDirectory.load(path)
    assert [r.to_dict() for r in loaded.records()] == [r.to_dict() for r in directory.records()]


def test_rehearsal_picks_the_cheapest_signed_match():
    result = rehearse()
    assert result["usdc_under_8"] == ["svc-atlas", "svc-aegean"]
    assert result["with_signed_descriptor"] == ["svc-atlas", "svc-boreas"]
    assert result["resolved_endpoint"] == "tcp://atlas.example:7443"
