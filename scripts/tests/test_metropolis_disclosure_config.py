"""Disclosure deployment configs carry the five runtime pins and match the schema the packages were built with."""

import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).resolve().parent / "fixtures"
DEPLOYMENT = ROOT / "contracts/evm/deployments/monad-testnet-shielded-v2.json"


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


config = load("metropolis_disclosure_config", "mcp-server/src/erebus_mcp/metropolis_setup/disclosure_config.py")
registry = load("metropolis_registry_schema", "scripts/build-metropolis-registry.py")
PIN = "0" * 63 + "1"


def record(tmp_path, pins=None):
    path = tmp_path / "deployment.json"
    path.write_text(json.dumps({"chain_id": 10143, "test_only": True, "pools": [
        {"name": name, "pool": pool, "namespace": "eip155:10143", "verifier_version": 2,
         "deployment_block": 7, "deployment_hash": "0x" + "ab" * 32,
         "runtime_pins": pins or {key: PIN for key in ("pool", "poseidon", "deposit", "transfer", "withdraw")}}
        for name, pool in (("wmon", "0x" + "88" * 20), ("usdc", "0x" + "86" * 20))]}))
    return path


def release():
    return {"deployment_schemas": registry.deployment_schemas()}


def rendered(tmp_path, pool="wmon"):
    return config.render(record(tmp_path), pool, "https://a.example.net/", "https://b.example.net/", "/cache")


def test_recorded_schema_is_the_strict_disclosure_struct_with_five_runtime_pins():
    schema = registry.deployment_schemas()["erebus-shielded-disclosure"]
    assert schema["strict"] is True
    for field in config.PINS:
        assert field in schema["fields"]
    assert {"namespace", "settlement_contract", "verifier_version", "first_block", "first_hash"} <= set(schema["fields"])


@pytest.mark.parametrize("pool", ["wmon", "usdc"])
def test_fixtures_have_exactly_the_packaged_fields_and_match_the_deployment(pool):
    fixture = json.loads((FIXTURES / f"metropolis-disclosure-deployment-{pool}.json").read_text())
    assert set(fixture) == set(registry.deployment_schemas()["erebus-shielded-disclosure"]["fields"])
    assert fixture["rpc_url"].endswith(".invalid/") and fixture["peer_rpc_url"].endswith(".invalid/")
    if not DEPLOYMENT.is_file():
        pytest.skip("deployment record is not in this revision")
    assert config.check(fixture, release(), DEPLOYMENT, pool)["accepted"]


def test_render_copies_the_pool_identity_and_requires_operator_endpoints(tmp_path):
    result = rendered(tmp_path, "usdc")
    assert result["settlement_contract"] == "0x" + "86" * 20 and result["first_block"] == 7
    assert all(result[field] == "0x" + PIN for field in config.PINS)
    assert config.check(result, release(), tmp_path / "deployment.json", "usdc")["accepted"]
    with pytest.raises(ValueError, match="must differ"):
        config.render(tmp_path / "deployment.json", "wmon", "https://a.example.net/", "https://a.example.net/", "/c")
    with pytest.raises(ValueError, match="credentials"):
        config.render(tmp_path / "deployment.json", "wmon", "https://u:p@a.example.net/", "https://b.example.net/", "/c")


def test_render_refuses_a_missing_or_zero_runtime_pin(tmp_path):
    pins = {key: PIN for key in ("pool", "poseidon", "deposit", "transfer")}
    with pytest.raises(KeyError):
        config.render(record(tmp_path, pins), "wmon", "https://a.example.net/", "https://b.example.net/", "/c")
    with pytest.raises(ValueError, match="non-zero"):
        config.render(record(tmp_path, dict(pins, withdraw="0" * 64)), "wmon",
                      "https://a.example.net/", "https://b.example.net/", "/c")


def test_packages_without_the_recorded_schema_are_refused(tmp_path):
    result = config.check(rendered(tmp_path), {"version": "0.3.0.dev2026101101"})
    assert not result["accepted"] and "did not record the disclosure deployment schema" in result["problems"][0]


def test_old_or_extended_configs_and_noncanonical_pins_are_refused(tmp_path):
    current = rendered(tmp_path)
    old = {key: value for key, value in current.items() if key not in
           ("poseidon_keccak256", "deposit_verifier_keccak256", "transfer_verifier_keccak256", "withdraw_verifier_keccak256")}
    assert "missing" in " ".join(config.check(old, release())["problems"])
    assert "unknown ['max_scan_blocks']" in " ".join(config.check(dict(current, max_scan_blocks=10), release())["problems"])
    for value in (PIN, "0x" + "0" * 64, "0x" + PIN.upper().replace("1", "A")):
        problems = config.check(dict(current, transfer_verifier_keccak256=value), release())["problems"]
        assert any("transfer_verifier_keccak256" in problem for problem in problems)
    same = dict(current, peer_rpc_url=current["rpc_url"])
    assert "endpoints must differ" in " ".join(config.check(same, release())["problems"])


def test_a_config_for_one_pool_is_refused_for_the_other(tmp_path):
    wmon = rendered(tmp_path, "wmon")
    problems = " ".join(config.check(wmon, release(), tmp_path / "deployment.json", "usdc")["problems"])
    assert "settlement_contract does not match the deployment record's usdc pool" in problems
    pinned = dict(wmon, deposit_verifier_keccak256="0x" + "0" * 63 + "2")
    assert "deposit_verifier_keccak256 does not match" in " ".join(
        config.check(pinned, release(), tmp_path / "deployment.json", "wmon")["problems"])


def test_schema_extraction_fails_closed_when_the_struct_is_missing(tmp_path):
    (tmp_path / "sdk/shielded/src/bin").mkdir(parents=True)
    (tmp_path / "sdk/shielded/src/bin/erebus_shielded_disclosure.rs").write_text("struct Other {}\n")
    with pytest.raises(ValueError, match="schema not found"):
        registry.deployment_schemas(tmp_path)
