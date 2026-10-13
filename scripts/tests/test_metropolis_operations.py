"""Installed-init and operations rehearsals restore operator state and never touch keys or chains on their own."""

import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


rehearse = load("metropolis_rehearse_operations", "scripts/rehearse-metropolis-operations.py")
init_check = load("metropolis_init_check", "scripts/check-metropolis-init.py")


def operator(tmp_path):
    directory = tmp_path / "wmon" / "buyer"
    directory.mkdir(parents=True)
    for name in ("wallet.json", "payment.json"):
        (directory / name).write_text(json.dumps({"rpc_url": "https://a.example.net/", "peer_rpc_url": "https://b.example.net/"}))
    return directory


def test_endpoint_change_restores_configs_even_when_doctor_fails(tmp_path, monkeypatch):
    directory = operator(tmp_path)
    before = {name: (directory / name).read_text() for name in ("wallet.json", "payment.json")}
    seen = []

    def failing(bins, work, operator_path, offline=True):
        seen.append(json.loads((operator_path / "wallet.json").read_text())["rpc_url"])
        raise OSError("endpoint down")

    monkeypatch.setattr(rehearse, "doctor", failing)
    with pytest.raises(OSError):
        rehearse.endpoint_change(tmp_path / "bin", tmp_path, directory)
    assert seen == ["https://b.example.net/"]
    assert {name: (directory / name).read_text() for name in before} == before


def test_endpoint_change_requires_both_endpoints_to_pass(tmp_path, monkeypatch):
    directory = operator(tmp_path)
    monkeypatch.setattr(rehearse, "doctor", lambda *a, **k: {"status": "attention", "problems": [],
                        "checks": {"endpoints": {"rpc_url": {"passed": True}, "peer_rpc_url": {"passed": False}}}})
    monkeypatch.setattr(rehearse, "run", lambda *a, **k: {"status": "ok"})
    with pytest.raises(ValueError, match="after the endpoint change"):
        rehearse.endpoint_change(tmp_path / "bin", tmp_path, directory)
    assert json.loads((directory / "wallet.json").read_text())["rpc_url"] == "https://a.example.net/"


def test_init_check_uses_two_distinct_public_rpc_providers_and_no_key_inputs():
    assert len(set(init_check.RPC)) == 2 and all(url.startswith("https://") for url in init_check.RPC)
    source = (ROOT / "scripts/check-metropolis-init.py").read_text()
    assert "--transaction-key-file" not in source and "eth_send" not in source
