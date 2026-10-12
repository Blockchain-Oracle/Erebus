"""One-step operator setup writes exactly the strict configs the installed commands parse, and refuses drift."""

import hashlib
import importlib.util
import json
import re
import stat
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("metropolis_operator_init", ROOT / "packaging/metropolis/operator_init.py")
init = importlib.util.module_from_spec(spec)
spec.loader.exec_module(init)

PIN = "0" * 63 + "1"
POOLS = {"wmon": ("0x" + "88" * 20, "0x" + "fb" * 20), "usdc": ("0x" + "86" * 20, "0x" + "53" * 20)}

FAKE_NEGOTIATE = r'''#!PYTHON
import json, os, sys
from pathlib import Path
request = json.loads(sys.stdin.read())
def private(path, data):
    Path(path).parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    Path(path).write_bytes(data); os.chmod(path, 0o600)
method = request["method"]
if method == "prepare_operator":
    d = Path(request["directory"]); reply = {"status": "ok", "suite_id": 2}
    for name in ("agreement", "wallet", "discovery", "transport") + (("spend",) if request["role"] == "seller" else ()):
        private(d / f"{name}.key", os.urandom(32))
    private(d / "agreement.public", os.urandom(64))
    (d / f"{request['role']}.descriptor.json").write_text(json.dumps({"role": request["role"], "endpoint": request["endpoint"]}))
    reply.update(agreement_key_file=str(d / "agreement.key"), wallet_agreement_key_file=str(d / "agreement.key"),
                 wallet_key_file=str(d / "wallet.key"), discovery_key_file=str(d / "discovery.key"),
                 transport_key_file=str(d / "transport.key"), descriptor_file=str(d / f"{request['role']}.descriptor.json"))
    if request["role"] == "seller":
        reply["seller_spend_secret_file"] = str(d / "spend.key")
elif method == "prepare_binding":
    private(request["output"], json.dumps(request).encode()); reply = {"status": "ok"}
elif method == "prepare_terms":
    private(request["output"], json.dumps(request).encode()); reply = {"status": "ok"}
else:
    reply = {"status": "error", "error": "unknown"}
print(json.dumps(reply)); sys.exit(0 if reply["status"] == "ok" else 1)
'''
FAKE_DISCLOSURE = r'''#!PYTHON
import json, os, sys
request = json.loads(sys.stdin.read())
fd = os.open(request["key_file"], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600); os.write(fd, os.urandom(32)); os.close(fd)
print(json.dumps({"status": "ok", "public_key": "00" * 32}))
'''


def rust_fields(relative: str, opener: str, within: str | None = None) -> set[str]:
    """Field names of a strict Rust struct or struct-like enum variant, read from the source."""
    source = (ROOT / relative).read_text()
    if within is not None:
        source = source[source.index(within):]
    match = re.search(re.escape(opener) + r"\s*\{(.*?)\n\s*\}", source, re.S)
    assert match, f"{opener} not found in {relative}"
    return set(re.findall(r"^\s+(?:pub\s+)?([a-z_][a-z0-9_]*)\s*:", match.group(1), re.M))


@pytest.fixture
def world(tmp_path, monkeypatch):
    monkeypatch.setattr(init, "wheel_tag", lambda: "macosx_11_0_arm64")
    bins = tmp_path / "bin"
    bins.mkdir()
    for name in init.COMMANDS:
        path = bins / name
        body = {"erebus-negotiate": FAKE_NEGOTIATE, "erebus-shielded-disclosure": FAKE_DISCLOSURE}.get(name, "#!/bin/sh\n")
        path.write_text(body.replace("#!PYTHON", f"#!{sys.executable}"))
        path.chmod(0o755)
    pins = tmp_path / "pins"
    pins.mkdir()
    manifests = {}
    for name, (pool, _) in POOLS.items():
        manifest = {"version": 2, "chain_id": 10143, "settlement_contract": pool, "verifier_version": 2, "test_only": True,
                    "circuits": [{"kind": kind, "identity": {"verifier_runtime_keccak256": PIN},
                                  **{field: {"sha256": hashlib.sha256(f"{kind}.{field}".encode()).hexdigest(), "bytes": 1}
                                     for field in ("wasm", "r1cs", "zkey")}} for kind in ("deposit", "transfer", "withdraw")]}
        (pins / f"manifest-{name}.json").write_text(json.dumps(manifest))
        (pins / f"manifest-{name}.signature.json").write_text(json.dumps({"version": 1, "signature_hex": "ab"}))
        manifests[name] = hashlib.sha256((pins / f"manifest-{name}.json").read_bytes()).hexdigest()
    binaries = {name: {"sha256": hashlib.sha256((bins / name).read_bytes()).hexdigest()} for name in init.COMMANDS}
    (pins / "release-manifest.json").write_text(json.dumps({"version": "0.3.0.dev2026101202", "source_commit": "07e6949",
        "platforms": {"macosx-11.0-arm64": {"platform_qualification": {"wheel_tag": "macosx_11_0_arm64"}, "binaries": binaries}}}))
    (pins / "testnet-release.json").write_text(json.dumps({"package_version": "0.3.0.dev2026101202",
        "publisher": "0x" + "cd" * 20, "hosting_manifests": manifests}))
    deployment = tmp_path / "deployment.json"
    deployment.write_text(json.dumps({"chain_id": 10143, "test_only": True, "pools": [
        {"name": name, "pool": pool, "asset": asset, "namespace": "eip155:10143", "verifier_version": 2,
         "deployment_block": 7, "deployment_hash": "0x" + "ab" * 32,
         "runtime_pins": {k: PIN for k in ("pool", "poseidon", "deposit", "transfer", "withdraw")}}
        for name, (pool, asset) in POOLS.items()]}))
    return SimpleNamespace(root=tmp_path, bins=bins, pins=pins, deployment=deployment)


def init_args(world, role, **changes):
    values = dict(role=role, pool="wmon", directory=world.root / role, bin=world.bins, pins=world.pins,
                  deployment=world.deployment, endpoint=f"127.0.0.1:{9101 if role == 'buyer' else 9102}",
                  rpc_url="https://a.example.net/", peer_rpc_url="https://b.example.net/", signer_address="0x" + "12" * 20,
                  transaction_key_file=None, maximum_price="800" if role == "buyer" else None,
                  descriptor_lifetime_seconds=86400, binding_lifetime_seconds=21600)
    values.update(changes)
    return SimpleNamespace(**values)


def connect_args(world, role, **changes):
    payload = world.root / "payload.json"
    payload.write_text("{}")
    values = dict(directory=world.root / role, peer_share=world.root / ("seller" if role == "buyer" else "buyer") / "share",
                  resource="dataset.snapshot.v1", amount="700" if role == "seller" else "600", minimum_price=None,
                  unit="snapshot", quantity="1", payload_file=payload if role == "seller" else None,
                  port=18732 if role == "seller" else None, delivery_seconds=21600)
    values.update(changes)
    return SimpleNamespace(**values)


def owner_only(path: Path) -> bool:
    return not stat.S_IMODE(path.stat().st_mode) & 0o077


def test_full_setup_writes_exactly_the_strict_rust_schemas(world):
    for role in ("buyer", "seller"):
        init.init(init_args(world, role))
    init.connect(connect_args(world, "seller"))
    init.connect(connect_args(world, "buyer"))
    buyer, seller = world.root / "buyer", world.root / "seller"
    shielded = "sdk/shielded/src/bin"
    expected = {
        buyer / "wallet.json": rust_fields(f"{shielded}/erebus_wallet.rs", "struct Config"),
        buyer / "payment.json": rust_fields(f"{shielded}/payment/shielded.rs", "struct ShieldedConfig"),
        buyer / "negotiate.json": rust_fields(f"{shielded}/erebus_negotiate.rs", "struct Config")
        - {"seller_spend_secret_file", "seller_wallet_file", "seller_wallet_key_file", "access_evidence_root"},
        seller / "negotiate.json": rust_fields(f"{shielded}/erebus_negotiate.rs", "struct Config"),
        seller / "access.json": rust_fields(f"{shielded}/erebus_access_service.rs", "struct Config")
        - {"profile", "max_response_bytes", "feed_file"},
    }
    for path, fields in expected.items():
        assert set(json.loads(path.read_text())) == fields, path.name
        assert owner_only(path), path.name
    backend = json.loads((seller / "access.json").read_text())["backend"]
    assert set(backend) - {"mode"} == rust_fields(f"{shielded}/erebus_access_service.rs", "Shielded", within="enum Backend")
    wallet = json.loads((buyer / "wallet.json").read_text())
    payment = json.loads((buyer / "payment.json").read_text())
    assert wallet["wrapper"] == POOLS["wmon"][1] and wallet["runtime_pins"]["withdraw"] == PIN
    assert wallet["artifact_manifest_authorization"]["publisher"] == "0x" + "cd" * 20
    assert payment["withdraw_verifier_keccak256"] == "0x" + PIN and payment["transaction_key_file"] is None
    assert len(payment["buyer_key_hex"]) == 128
    environment = json.loads((buyer / "mcp.json").read_text())["mcpServers"]["erebus-buyer"]["env"]
    assert environment["EREBUS_BACKEND"] == "metropolis" and environment["EREBUS_PAYMENT_CONFIG"].endswith("payment.json")
    assert sorted(p.name for p in (seller / "share").iterdir()) == ["identity.binding", "offer.json", "seller.descriptor.json"]
    assert not list((buyer / "share").glob("*.key"))


def test_init_refuses_tampered_binaries_unpinned_manifests_and_bad_inputs(world):
    (world.bins / "erebus-payment").write_text("#!/bin/sh\necho changed\n")
    with pytest.raises(ValueError, match="erebus-payment is not the published binary"):
        init.init(init_args(world, "buyer"))
    (world.bins / "erebus-payment").write_text("#!/bin/sh\n")
    (world.pins / "manifest-wmon.json").write_text("{}")
    with pytest.raises(ValueError, match="does not match the release authorization"):
        init.init(init_args(world, "buyer"))
    for change, message in ((dict(signer_address="0x" + "0" * 40), "own gas address"),
                            (dict(peer_rpc_url="https://a.example.net/"), "must differ"),
                            (dict(maximum_price=None), "--maximum-price"),
                            (dict(pool="usdc", directory=world.root / "u", rpc_url="http://u:p@x/"), "credentials")):
        with pytest.raises(ValueError, match=message):
            init.init(init_args(world, "buyer", **change))


def test_init_refuses_a_manifest_bound_to_another_pool(world):
    other = world.pins / "manifest-usdc.json"
    (world.pins / "manifest-wmon.json").write_bytes(other.read_bytes())
    authorization = json.loads((world.pins / "testnet-release.json").read_text())
    authorization["hosting_manifests"]["wmon"] = authorization["hosting_manifests"]["usdc"]
    (world.pins / "testnet-release.json").write_text(json.dumps(authorization))
    with pytest.raises(ValueError, match="names a different pool"):
        init.init(init_args(world, "buyer"))
    assert not (world.root / "buyer").exists()


def test_connect_requires_the_sellers_offer_and_unchanged_binaries(world):
    for role in ("buyer", "seller"):
        init.init(init_args(world, role))
    with pytest.raises(ValueError, match="offer.json"):
        init.connect(connect_args(world, "buyer"))
    init.connect(connect_args(world, "seller"))
    with pytest.raises(ValueError, match="different resource"):
        init.connect(connect_args(world, "buyer", resource="other"))
    (world.bins / "erebus-negotiate").write_text((world.bins / "erebus-negotiate").read_text() + "\n# changed\n")
    with pytest.raises(ValueError, match="erebus-negotiate is not the published binary"):
        init.connect(connect_args(world, "buyer"))


def test_auditor_setup_uses_the_installed_keygen_and_the_strict_disclosure_schema(world):
    init.auditor(SimpleNamespace(pool="usdc", directory=world.root / "auditor", bin=world.bins, deployment=world.deployment,
                                 rpc_url="https://a.example.net/", peer_rpc_url="https://b.example.net/"))
    config = json.loads((world.root / "auditor/disclosure.json").read_text())
    assert set(config) == rust_fields("sdk/shielded/src/bin/erebus_shielded_disclosure.rs", "struct Deployment")
    assert config["settlement_contract"] == POOLS["usdc"][0] and owner_only(world.root / "auditor/auditor.key")


def test_pins_downloads_over_https_from_the_release_commit_and_checks_manifests(tmp_path):
    import io
    commit = "07e6949" + "0" * 33
    manifest = b'{"settlement_contract": "0x88"}'
    authorization = {"package_version": "0.3.0.dev1", "artifact_tag": "artifacts-x", "publisher": "0x" + "cd" * 20,
                     "hosting_manifests": {"wmon": hashlib.sha256(manifest).hexdigest()}}
    served = {
        f"{init.REGISTRY}/v0.3.0.dev1/release-manifest.json": json.dumps({"version": "0.3.0.dev1", "source_commit": commit}).encode(),
        f"{init.SOURCE}/{commit}/packaging/metropolis/testnet-release.json": json.dumps(authorization).encode(),
        f"{init.SOURCE}/{commit}/contracts/evm/deployments/monad-testnet-shielded-v2.json": b"{}",
        f"{init.REGISTRY}/artifacts-x/manifest-wmon.json": manifest,
        f"{init.REGISTRY}/artifacts-x/manifest-wmon.signature.json": b"{}",
    }
    opener = lambda url, timeout=120: io.BytesIO(served[url])
    result = init.pins(SimpleNamespace(version="0.3.0.dev1", directory=tmp_path / "pins"), opener)
    assert result["source_commit"] == commit and sorted(result["files"]) == sorted(p.name for p in (tmp_path / "pins").iterdir())
    served[f"{init.REGISTRY}/artifacts-x/manifest-wmon.json"] = b'{"settlement_contract": "0x99"}'
    with pytest.raises(ValueError, match="differs from the authorization"):
        init.pins(SimpleNamespace(version="0.3.0.dev1", directory=tmp_path / "pins-2"), opener)
    with pytest.raises(ValueError, match="https"):
        init.https("http://example.net/x", opener)


def test_doctor_is_read_only_and_flags_drift_without_printing_secrets(world, monkeypatch):
    init.init(init_args(world, "buyer"))
    monkeypatch.setattr(init, "command", lambda bins, name, request, cwd: {"status": "ok", "result": {"notes": [], "operations": []}})
    healthy = init.doctor(SimpleNamespace(directory=world.root / "buyer", offline=True))
    assert healthy["status"] == "healthy" and healthy["secrets_printed"] is False
    assert healthy["checks"]["owner_only"]["problems"] == [] and healthy["checks"]["artifact_cache"]["of"] == 9
    rendered = json.dumps(healthy)
    for key in (world.root / "buyer/keys").glob("*.key"):
        assert key.read_bytes().hex() not in rendered
    (world.root / "buyer/wallet.json").chmod(0o644)
    (world.bins / "erebus-wallet").write_text("#!/bin/sh\necho changed\n")
    problems = " ".join(init.doctor(SimpleNamespace(directory=world.root / "buyer", offline=True))["problems"])
    assert "erebus-wallet is not the published binary" in problems and "wallet.json" in problems
