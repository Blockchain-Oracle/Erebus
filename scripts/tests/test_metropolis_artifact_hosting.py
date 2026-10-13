"""Artifact staging preserves every identity, refuses placeholder hosting, and never claims hosting itself."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("metropolis_stage_artifacts", ROOT / "mcp-server/src/erebus_mcp/metropolis_setup/stage_artifacts.py")
staging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(staging)
BASE = "https://github.com/PoulavBhowmick03/erebus-metropolis/releases/download/artifacts-test"


def bundle(tmp_path):
    root = tmp_path / "bundle"
    circuits = []
    for kind in staging.KINDS:
        key = root / f"{kind}-verification-key.json"
        key.parent.mkdir(parents=True, exist_ok=True)
        key.write_text(json.dumps({"circuit": kind}))
        record = {"kind": kind, "identity": {"verification_key_sha256": staging.sha256(key)}}
        for field in staging.FIELDS:
            path = root / staging.bundle_path(kind, field)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"{kind}-{field}".encode())
            record[field] = {"url": f"https://example.invalid/{kind}/{field}", "sha256": staging.sha256(path),
                             "bytes": path.stat().st_size}
        circuits.append(record)
    manifest = {"version": 2, "chain_id": 10143, "settlement_contract": "0x" + "12" * 20,
                "verifier_version": 2, "test_only": True, "circuits": circuits}
    (root / "manifest.json").write_text(json.dumps(manifest))
    return root, hashlib.sha256((root / "manifest.json").read_bytes()).hexdigest()


def test_staging_rewrites_only_urls_and_reports_a_new_unsigned_unhosted_manifest(tmp_path):
    root, pin = bundle(tmp_path)
    record = staging.stage(root, pin, BASE, tmp_path / "out")
    source = json.loads((root / "manifest.json").read_text())
    hosted = json.loads((tmp_path / "out/manifest.json").read_text())
    assert record["signed"] is False and record["hosted"] is False and record["test_only"] is True
    assert record["source_manifest_sha256"] == pin != record["hosting_manifest_sha256"]
    for before, after in zip(source["circuits"], hosted["circuits"]):
        assert before["identity"] == after["identity"]
        for field in staging.FIELDS:
            assert after[field]["url"] == f"{BASE}/{before['kind']}.{field}"
            assert {k: v for k, v in after[field].items() if k != "url"} == {k: v for k, v in before[field].items() if k != "url"}
            assert staging.sha256(tmp_path / "out/files" / f"{before['kind']}.{field}") == before[field]["sha256"]
    assert len((tmp_path / "out/SHA256SUMS").read_text().splitlines()) == 12  # nine artifacts, three public keys
    assert [key["name"] for key in record["verification_keys"]] == \
        [f"{kind}-verification-key.json" for kind in sorted(staging.KINDS)]


@pytest.mark.parametrize("url", [
    "https://example.invalid/x", "http://github.com/x", "https://localhost/x", "https://127.0.0.1/x",
    "https://10.0.0.1/x", "https://user:pw@github.com/x", "https://github.com/x?y=1", "https://example.com/x",
    "https://artifacts.test/x",
])
def test_placeholder_local_or_insecure_base_urls_are_refused(tmp_path, url):
    root, pin = bundle(tmp_path)
    with pytest.raises(ValueError):
        staging.stage(root, pin, url, tmp_path / "out")
    assert not (tmp_path / "out").exists()


def test_wrong_pin_or_changed_file_is_refused(tmp_path):
    root, pin = bundle(tmp_path)
    with pytest.raises(ValueError, match="pin"):
        staging.stage(root, "0" * 64, BASE, tmp_path / "out")
    (root / "transfer.zkey").write_bytes(b"tampered")
    with pytest.raises(ValueError, match="transfer.zkey"):
        staging.stage(root, pin, BASE, tmp_path / "out2")


def test_verify_reports_hosted_only_when_every_download_matches(tmp_path):
    root, pin = bundle(tmp_path)
    staging.stage(root, pin, BASE, tmp_path / "out")
    files = tmp_path / "out/files"

    class Response(io.BytesIO):
        def __enter__(self):
            return self

        def __exit__(self, *args):
            return False

    def serve(url, timeout):
        assert url.get_header("User-agent")  # every hosted download names its client
        url = url.full_url
        return Response((files / url.rsplit("/", 1)[1]).read_bytes())

    assert staging.verify(tmp_path / "out", serve)["hosted"] is True

    def tampered(url, timeout):
        url = url.full_url
        return Response(b"x" if url.endswith("transfer.zkey") else (files / url.rsplit("/", 1)[1]).read_bytes())

    result = staging.verify(tmp_path / "out", tampered)
    assert result["hosted"] is False
    assert [entry["url"] for entry in result["files"] if not entry["ok"]] == [f"{BASE}/transfer.zkey"]


checker_spec = importlib.util.spec_from_file_location("metropolis_hosted_checker", ROOT / "scripts/check-metropolis-install.py")
checker = importlib.util.module_from_spec(checker_spec)
checker_spec.loader.exec_module(checker)


def hosted_manifest(tmp_path, base=BASE):
    root, pin = bundle(tmp_path)
    staged = tmp_path / "staged"
    record = staging.stage(root, pin, base, staged)
    return staged / "manifest.json", record["hosting_manifest_sha256"]


def test_hosted_check_requires_the_pinned_manifest_the_release_recorded(tmp_path):
    manifest, pin = hosted_manifest(tmp_path)
    signature = tmp_path / "signature.json"
    signature.write_text("{}")
    args = (tmp_path / "bin", None, manifest, pin, "0x" + "12" * 20, signature, tmp_path, {}, tmp_path, 60)
    with pytest.raises(ValueError, match="pin"):
        checker.rehearse_hosted(args[0], {"proving_artifacts": {"manifest_sha256": pin}}, manifest, "0" * 64, *args[4:])
    with pytest.raises(ValueError, match="did not record"):
        checker.rehearse_hosted(args[0], {"proving_artifacts": {"manifest_sha256": "1" * 64}}, manifest, pin, *args[4:])


def test_hosted_check_refuses_placeholder_urls_even_when_pinned_and_recorded(tmp_path):
    manifest, _ = hosted_manifest(tmp_path)
    data = json.loads(manifest.read_text())
    data["circuits"][0]["zkey"]["url"] = "https://example.invalid/deposit.zkey"
    manifest.write_text(json.dumps(data))
    pin = hashlib.sha256(manifest.read_bytes()).hexdigest()
    signature = tmp_path / "signature.json"
    signature.write_text("{}")
    with pytest.raises(ValueError, match="placeholder"):
        checker.rehearse_hosted(tmp_path, {"proving_artifacts": {"manifest_sha256": pin}}, manifest, pin,
                                "0x" + "12" * 20, signature, tmp_path, {}, tmp_path, 60)


def test_redirecting_release_server_mimics_release_asset_downloads(tmp_path):
    import urllib.request
    (tmp_path / "objects").mkdir()
    (tmp_path / "objects/deposit.zkey").write_bytes(b"zkey")
    server, thread, handler = checker.serve_release(tmp_path)
    try:
        url = f"http://127.0.0.1:{server.server_port}/releases/download/tag/deposit.zkey"
        with urllib.request.urlopen(url, timeout=5) as response:
            assert response.read() == b"zkey"
            assert response.geturl().endswith("/objects/deposit.zkey")
        assert handler.redirects == 1
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def fake_prover(tmp_path, monkeypatch, behaviour):
    """Model the installer's cache: verified files are reused, missing or mismatched ones download.
    Like the installed prover, refuse public inputs from another domain before installing anything."""
    def install(bins, release, request, environment, timeout, prove=False):
        payload = json.loads(request.read_text())
        manifest = json.loads(Path(payload["manifest_file"]).read_bytes())
        if behaviour(payload, manifest):
            raise ValueError("installed proving qualification failed")
        domain = [str(manifest["chain_id"]), str(int(manifest["settlement_contract"], 16)), str(manifest["verifier_version"])]
        if prove and payload["expected_public"][:3] != domain:
            raise ValueError("installed proving qualification failed")
        circuit = next(c for c in manifest["circuits"] if c["kind"] == payload["circuit"])
        cache = Path(payload["cache_root"])
        cache.mkdir(mode=0o700, exist_ok=True)
        downloaded = hits = 0
        for field in staging.FIELDS:
            path = cache / f"{circuit[field]['sha256']}.artifact"
            if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest() == circuit[field]["sha256"]:
                hits += 1
            else:
                path.write_bytes((tmp_path / "bundle" / staging.bundle_path(circuit["kind"], field)).read_bytes())
                downloaded += circuit[field]["bytes"]
        return {"verified": True, "test_only": manifest["test_only"], "downloaded_bytes": downloaded, "cache_hits": hits}
    calls = {"proofs": 0, "installs": 0}

    def qualify(*args, check_recorded=True):
        calls["proofs"] += 1
        return install(*args, prove=True)

    def install_only(*args):
        calls["installs"] += 1
        return install(*args)
    monkeypatch.setattr(checker, "qualify_installed_prover", qualify)
    monkeypatch.setattr(checker, "install_artifacts", install_only)
    return calls


def witness_directory(tmp_path, contract="0x" + "12" * 20):
    """Fixture witnesses bound to (10143, contract, 2); the bundle() manifest names pool 0x1212...12."""
    witnesses = tmp_path / "witnesses"
    witnesses.mkdir()
    for kind, fields in checker.TRANSITIONS.items():
        values = {field: "1" for field in fields}
        values.update(chainId="10143", contractAddress=str(int(contract, 16)), verifierVersion="2")
        (witnesses / f"{kind}-input.json").write_text(json.dumps(values))
    return witnesses


def test_hosted_sequence_redownloads_tampered_cache_and_requires_authorization_refusals(tmp_path, monkeypatch):
    manifest, _ = hosted_manifest(tmp_path)
    authorization = {"publisher": "0x" + "34" * 20, "signature": {}}

    def enforce(payload, manifest_data):
        return (payload["manifest_authorization"]["publisher"] != authorization["publisher"]
                or Path(payload["manifest_file"]).name == "changed-manifest.json")

    fake_prover(tmp_path, monkeypatch, enforce)
    work = tmp_path / "work"
    work.mkdir()
    report = checker.prove_against_manifest(tmp_path, {}, manifest, authorization, witness_directory(tmp_path),
                                            {}, work, 60, loopback=False)
    assert report["cache_tamper"] == "redownloaded" and report["proved"] is True
    assert report["authorization_refusals"] == {"wrong_publisher": True, "changed_manifest_bytes": True,
                                                "foreign_domain_public_inputs": True}
    assert report["transitions"]["transfer"]["warm"]["cache_hits"] == 3


def test_foreign_domain_witnesses_install_without_proving_and_must_be_refused(tmp_path, monkeypatch):
    manifest, _ = hosted_manifest(tmp_path)
    authorization = {"publisher": "0x" + "34" * 20, "signature": {}}
    calls = fake_prover(tmp_path, monkeypatch, lambda payload, manifest_data:
                        payload["manifest_authorization"]["publisher"] != authorization["publisher"]
                        or Path(payload["manifest_file"]).name == "changed-manifest.json")
    work = tmp_path / "work"
    work.mkdir()
    report = checker.prove_against_manifest(tmp_path, {}, manifest, authorization,
                                            witness_directory(tmp_path, "0x" + "5f" * 20), {}, work, 60, loopback=False)
    assert report["proved"] is False and report["witness_domain"]["settlement_contract"] == "0x" + "5f" * 20
    assert report["authorization_refusals"] == {"wrong_publisher": True, "changed_manifest_bytes": True,
                                                "foreign_domain_witness": True}
    assert report["cache_tamper"] == "redownloaded" and calls["proofs"] == 1 and calls["installs"] >= 5
    assert not any((work / "refusal-cache-domain").glob("*.artifact"))


def test_a_prover_accepting_foreign_domain_witnesses_fails_the_check(tmp_path, monkeypatch):
    manifest, _ = hosted_manifest(tmp_path)
    authorization = {"publisher": "0x" + "34" * 20, "signature": {}}
    fake_prover(tmp_path, monkeypatch, lambda payload, manifest_data:
                payload["manifest_authorization"]["publisher"] != authorization["publisher"]
                or Path(payload["manifest_file"]).name == "changed-manifest.json")
    monkeypatch.setattr(checker, "qualify_installed_prover", lambda *args, **kwargs: {"verified": True})
    work = tmp_path / "work"
    work.mkdir()
    with pytest.raises(ValueError, match="another domain"):
        checker.prove_against_manifest(tmp_path, {}, manifest, authorization,
                                       witness_directory(tmp_path, "0x" + "5f" * 20), {}, work, 60, loopback=False)


def test_hosted_sequence_fails_when_authorization_is_not_enforced(tmp_path, monkeypatch):
    manifest, _ = hosted_manifest(tmp_path)
    fake_prover(tmp_path, monkeypatch, lambda payload, manifest_data: False)
    work = tmp_path / "work"
    work.mkdir()
    with pytest.raises(ValueError, match="authorization was not enforced"):
        checker.prove_against_manifest(tmp_path, {}, manifest, {"publisher": "0x" + "34" * 20, "signature": {}},
                                       witness_directory(tmp_path), {}, work, 60, loopback=False)


def test_hosted_fixture_requires_the_bundle_flags():
    with pytest.raises(ValueError, match="hosted fixture requires"):
        checker.check(Path("/nonexistent"), hosted_fixture=True)


GOOD_PUBLISHER = "0x" + "56" * 20
REHEARSAL_POOL = "0x" + "12" * 20


def fake_installer(tmp_path, accepted=GOOD_PUBLISHER):
    """Stand-in for installed erebus-artifacts: accepts one publisher and reports cache-only installs."""
    script = tmp_path / "erebus-artifacts"
    script.write_text(f"""#!{__import__('sys').executable}
import json, sys
request = json.loads(sys.stdin.read())
ok = request["manifest_authorization"]["publisher"] == {accepted!r}
print(json.dumps({{"status": "ok", "downloaded_bytes": 0, "cache_hits": 3}} if ok else {{"status": "error"}}))
sys.exit(0 if ok else 1)
""")
    script.chmod(0o755)
    return script


def staged_for_preflight(tmp_path, base=BASE):
    root, pin = bundle(tmp_path)
    staged = tmp_path / "staged"
    record = staging.stage(root, pin, base, staged)
    signature = staged / "manifest.signature.json"
    signature.write_text("{}")
    return staged, signature, record


def release_file(tmp_path, name, manifest_sha256, eligible=False, test_only=True):
    path = tmp_path / name
    path.write_text(json.dumps({"platform": name, "candidate": {"release_eligible": eligible},
                                "proving_artifacts": {"manifest_sha256": manifest_sha256, "test_only": test_only}}))
    return path


def deployment_file(tmp_path, pool=REHEARSAL_POOL, runtime="0" * 63 + "1"):
    path = tmp_path / "deployment.json"
    path.write_text(json.dumps({"namespace": "eip155:10143", "pool": pool, "verifier_version": 2,
                                "runtime_pins": {name: runtime for name in ("pool", "poseidon", "deposit", "transfer", "withdraw")}}))
    return path


def test_preflight_passes_a_signed_rehearsal_bundle_but_keeps_labels_and_ineligible_packages(tmp_path):
    staged, signature, record = staged_for_preflight(tmp_path)
    releases = [release_file(tmp_path, "macos.json", record["hosting_manifest_sha256"]),
                release_file(tmp_path, "linux.json", record["hosting_manifest_sha256"])]
    result = staging.preflight(staged, GOOD_PUBLISHER, signature, fake_installer(tmp_path), None, REHEARSAL_POOL, releases)
    assert result["problems"] == [] and result["artifacts_publishable"] is True
    assert result["labels"] == ["test-only", "unaudited", "single-operator", "rehearsal-domain"]
    assert result["packages_publishable"] is False and result["uploaded"] is False


def test_preflight_rejects_missing_or_invalid_signatures_and_invented_publishers(tmp_path):
    staged, signature, _ = staged_for_preflight(tmp_path)
    installer = fake_installer(tmp_path)
    assert "signature is missing" in " ".join(staging.preflight(
        staged, GOOD_PUBLISHER, tmp_path / "absent.json", installer, None, REHEARSAL_POOL, [])["problems"])
    assert "rejected" in " ".join(staging.preflight(
        staged, "0x" + "78" * 20, signature, installer, None, REHEARSAL_POOL, [])["problems"])
    for publisher in ("0x" + "0" * 40, "0xABC", "publisher"):
        assert not staging.preflight(staged, publisher, signature, installer, None, REHEARSAL_POOL, [])["artifacts_publishable"]


def test_preflight_rejects_placeholder_urls_written_after_staging(tmp_path):
    staged, signature, _ = staged_for_preflight(tmp_path)
    data = json.loads((staged / "manifest.json").read_text())
    data["circuits"][1]["zkey"]["url"] = "https://example.invalid/transfer.zkey"
    (staged / "manifest.json").write_text(json.dumps(data))
    problems = " ".join(staging.preflight(staged, GOOD_PUBLISHER, signature, fake_installer(tmp_path), None,
                                          REHEARSAL_POOL, [])["problems"])
    assert "placeholder" in problems and "changed after staging" in problems and "one release base" in problems


def test_preflight_rejects_rehearsal_domain_mismatch_and_ambiguous_domain(tmp_path):
    staged, signature, _ = staged_for_preflight(tmp_path)
    installer = fake_installer(tmp_path)
    assert "rehearsal domain" in " ".join(staging.preflight(
        staged, GOOD_PUBLISHER, signature, installer, None, "0x" + "99" * 20, [])["problems"])
    assert "exactly one" in " ".join(staging.preflight(
        staged, GOOD_PUBLISHER, signature, installer, None, None, [])["problems"])
    assert "exactly one" in " ".join(staging.preflight(
        staged, GOOD_PUBLISHER, signature, installer, deployment_file(tmp_path), REHEARSAL_POOL, [])["problems"])


def test_preflight_rejects_incompatible_deployment_and_package_identities(tmp_path):
    staged, signature, record = staged_for_preflight(tmp_path)
    installer = fake_installer(tmp_path)
    other_pool = staging.preflight(staged, GOOD_PUBLISHER, signature, installer,
                                   deployment_file(tmp_path, pool="0x" + "34" * 20), None, [])["problems"]
    assert any("settlement_contract" in problem for problem in other_pool)
    assert sum("verifier runtime" in problem for problem in other_pool) == 3
    stale = release_file(tmp_path, "stale.json", record["source_manifest_sha256"])
    flipped = release_file(tmp_path, "flipped.json", record["hosting_manifest_sha256"], test_only=False)
    problems = " ".join(staging.preflight(staged, GOOD_PUBLISHER, signature, installer, None, REHEARSAL_POOL,
                                          [stale, flipped])["problems"])
    assert "stale.json: packages did not record this proving manifest" in problems
    assert "flipped.json: packages disagree about the fixture boundary" in problems


def test_preflight_matching_deployment_still_requires_eligible_packages(tmp_path):
    root, pin = bundle(tmp_path)
    manifest = json.loads((root / "manifest.json").read_text())
    for circuit in manifest["circuits"]:
        circuit["identity"]["verifier_runtime_keccak256"] = "0" * 63 + "1"
    (root / "manifest.json").write_text(json.dumps(manifest))
    pin = hashlib.sha256((root / "manifest.json").read_bytes()).hexdigest()
    record = staging.stage(root, pin, BASE, tmp_path / "staged")
    signature = tmp_path / "staged/manifest.signature.json"
    signature.write_text("{}")
    release = release_file(tmp_path, "macos.json", record["hosting_manifest_sha256"], eligible=False)
    result = staging.preflight(tmp_path / "staged", GOOD_PUBLISHER, signature, fake_installer(tmp_path),
                               deployment_file(tmp_path), None, [release])
    assert result["problems"] == [] and result["domain"] == "deployment"
    assert result["labels"] == ["test-only", "unaudited", "single-operator"] and result["packages_publishable"] is False


POOLS = {"wmon": "0x" + "88" * 20, "usdc": "0x" + "86" * 20}
PIN = "0" * 63 + "1"


def pool_bundle(tmp_path, name):
    """A domain-bound bundle whose verifier identities carry the deployment pins, plus its source."""
    root, _ = bundle(tmp_path / name)
    manifest = json.loads((root / "manifest.json").read_text())
    for circuit in manifest["circuits"]:
        key = root / f"{circuit['kind']}-verification-key.json"
        key.write_text("{}")
        circuit["identity"] = {"verification_key_sha256": staging.sha256(key), "verifier_runtime_keccak256": PIN}
    source = tmp_path / name / "source-manifest.json"
    source.write_text(json.dumps(manifest))
    manifest["settlement_contract"] = POOLS[name]
    (root / "manifest.json").write_text(json.dumps(manifest))
    return root, hashlib.sha256((root / "manifest.json").read_bytes()).hexdigest(), source


def deployment_record(tmp_path, source_sha256=None, independent=None):
    record = {"chain_id": 10143, "test_only": True, "audited": False, "manifest_sha256": source_sha256,
              "pools": [{"name": name, "pool": pool, "namespace": "eip155:10143", "verifier_version": 2,
                         "runtime_pins": {key: PIN for key in ("pool", "poseidon", "deposit", "transfer", "withdraw")}}
                        for name, pool in POOLS.items()]}
    if independent is not None:
        record["independent"] = independent
    path = tmp_path / "deployment-record.json"
    path.write_text(json.dumps(record))
    return path


def test_multi_pool_deployment_record_requires_one_named_pool(tmp_path):
    path = deployment_record(tmp_path)
    assert staging.deployment_identity(path, "usdc")["settlement_contract"] == POOLS["usdc"]
    for name in (None, "dai"):
        with pytest.raises(ValueError, match="exactly one"):
            staging.deployment_identity(path, name)
    with pytest.raises(ValueError, match="pools list"):
        staging.deployment_identity(deployment_file(tmp_path), "wmon")


def test_bundle_check_binds_each_bundle_to_its_own_pool_and_recorded_source(tmp_path):
    root, pin, source = pool_bundle(tmp_path, "wmon")
    source_sha256 = staging.sha256(source)
    record = deployment_record(tmp_path, source_sha256)
    result = staging.check_bundle(root, pin, record, "wmon", source, source_sha256)
    assert result["matches_deployment"] and result["source"]["changed_fields"] == ["settlement_contract"]
    assert any("settlement_contract" in problem for problem in staging.check_bundle(root, pin, record, "usdc")["problems"])
    other = deployment_record(tmp_path, "f" * 64)
    assert "not the manifest the deployment recorded" in " ".join(
        staging.check_bundle(root, pin, other, "wmon", source, source_sha256)["problems"])
    with pytest.raises(ValueError, match="pin"):
        staging.check_bundle(root, "0" * 64, record, "wmon")


def test_bundle_check_refuses_runtime_key_and_identity_drift(tmp_path):
    root, _, source = pool_bundle(tmp_path, "usdc")
    manifest = json.loads((root / "manifest.json").read_text())
    manifest["circuits"][1]["identity"]["verifier_runtime_keccak256"] = "0" * 63 + "2"
    manifest["verifier_version"] = 3
    (root / "manifest.json").write_text(json.dumps(manifest))
    (root / "withdraw-verification-key.json").write_text('{"changed": true}')
    pin = staging.sha256(root / "manifest.json")
    problems = " ".join(staging.check_bundle(root, pin, deployment_record(tmp_path), "usdc", source,
                                             staging.sha256(source))["problems"])
    assert "transfer verifier runtime" in problems and "verifier_version" in problems
    assert "withdraw verification key" in problems and "more than the domain" in problems
    assert "rebinding changed an artifact or circuit identity" in problems


def test_pool_domains_share_artifact_bytes_but_get_separate_manifest_assets(tmp_path):
    records = {}
    for name in POOLS:
        root, pin, _ = pool_bundle(tmp_path, name)
        records[name] = staging.stage(root, pin, BASE, tmp_path / f"staged-{name}", name)
    assert records["wmon"]["files"] == records["usdc"]["files"]
    assert records["wmon"]["hosting_manifest_sha256"] != records["usdc"]["hosting_manifest_sha256"]
    assert (records["usdc"]["manifest_asset"], records["usdc"]["signature_asset"]) == \
        ("manifest-usdc.json", "manifest-usdc.signature.json")
    for bad in ("", "USDC", "a/b", "x" * 33):
        with pytest.raises(ValueError, match="domain name"):
            staging.stage(tmp_path / "wmon/bundle", records["wmon"]["source_manifest_sha256"], BASE, tmp_path / "bad", bad)


def test_preflight_selects_the_pool_and_accepts_packages_recording_both_domains(tmp_path):
    staged, pins = {}, {}
    for name in POOLS:
        root, pin, _ = pool_bundle(tmp_path, name)
        pins[name] = staging.stage(root, pin, BASE, tmp_path / f"staged-{name}", name)["hosting_manifest_sha256"]
        staged[name] = tmp_path / f"staged-{name}"
        (staged[name] / "manifest.signature.json").write_text("{}")
    release = tmp_path / "release.json"
    release.write_text(json.dumps({"platform": "macos", "candidate": {"release_eligible": True}, "proving_artifacts": None,
                                   "proving_domains": [{"manifest_sha256": pins[name], "test_only": True} for name in POOLS]}))
    installer, record = fake_installer(tmp_path), deployment_record(tmp_path)
    for name in POOLS:
        result = staging.preflight(staged[name], GOOD_PUBLISHER, staged[name] / "manifest.signature.json", installer,
                                   record, None, [release], name)
        assert result["problems"] == [] and result["settlement_contract"] == POOLS[name]
        assert result["labels"] == ["test-only", "unaudited", "single-operator"] and result["packages_publishable"]
    crossed = staging.preflight(staged["wmon"], GOOD_PUBLISHER, staged["wmon"] / "manifest.signature.json", installer,
                                record, None, [], "usdc")
    assert any("settlement_contract" in problem for problem in crossed["problems"])
    independent = staging.preflight(staged["usdc"], GOOD_PUBLISHER, staged["usdc"] / "manifest.signature.json", installer,
                                    deployment_record(tmp_path, independent=True), None, [], "usdc")
    assert independent["labels"] == ["test-only", "unaudited"]


def test_preflight_refuses_a_production_manifest_for_a_test_only_deployment(tmp_path):
    root, _, _ = pool_bundle(tmp_path, "wmon")
    manifest = json.loads((root / "manifest.json").read_text())
    manifest["test_only"] = False
    (root / "manifest.json").write_text(json.dumps(manifest))
    staging.stage(root, staging.sha256(root / "manifest.json"), BASE, tmp_path / "staged", "wmon")
    (tmp_path / "staged/manifest.signature.json").write_text("{}")
    problems = staging.preflight(tmp_path / "staged", GOOD_PUBLISHER, tmp_path / "staged/manifest.signature.json",
                                 fake_installer(tmp_path), deployment_record(tmp_path), None, [], "wmon")["problems"]
    assert "a test-only deployment requires a test-only manifest" in problems


def test_hosted_checks_accept_any_recorded_domain_and_label_unrecorded_fixture_domains(tmp_path):
    manifest, pin = hosted_manifest(tmp_path)
    release = {"proving_artifacts": None, "proving_domains": [{"manifest_sha256": "1" * 64}, {"manifest_sha256": pin}]}
    assert checker.recorded_proving_sha256(release) == {"1" * 64, pin}
    signature = tmp_path / "signature.json"
    signature.write_text("{}")
    with pytest.raises(FileNotFoundError, match="deposit-input.json"):  # past the record check, no witnesses here
        checker.rehearse_hosted(tmp_path, release, manifest, pin, "0x" + "12" * 20, signature, tmp_path, {}, tmp_path, 60)
    root, _ = bundle(tmp_path / "unrecorded")
    with pytest.raises(ValueError, match="release recorded"):
        checker.rehearse_hosted_fixture(tmp_path, release, root, tmp_path, {}, tmp_path, 60)
    with pytest.raises(ValueError, match="requires --hosted-fixture"):
        checker.check(Path("/nonexistent"), proving_witnesses=tmp_path, hosted_fixture_bundles=[root])


def serve_staged(staged):
    """An opener answering staged release-asset URLs from the staged files directory."""
    def opener(url, timeout=60):
        url = getattr(url, "full_url", url)
        return io.BytesIO((staged / "files" / url.rsplit("/", 1)[1]).read_bytes())
    return opener


def test_staging_refuses_a_verification_key_that_differs_from_the_identity(tmp_path):
    root, pin = bundle(tmp_path)
    (root / "transfer-verification-key.json").write_text("{}")
    with pytest.raises(ValueError, match="verification key"):
        staging.stage(root, pin, BASE, tmp_path / "out")


def test_fetch_rebuilds_the_bundle_layout_from_hosted_assets_and_checks_every_digest(tmp_path):
    root, pin = bundle(tmp_path)
    record = staging.stage(root, pin, BASE, tmp_path / "staged", "wmon")
    manifest = tmp_path / "staged/manifest.json"
    result = staging.fetch(manifest, record["hosting_manifest_sha256"], tmp_path / "fetched", serve_staged(tmp_path / "staged"))
    assert result["hosted"] and len(result["files"]) == 12
    for kind in staging.KINDS:
        assert (tmp_path / "fetched" / f"{kind}-verification-key.json").read_bytes() == \
            (root / f"{kind}-verification-key.json").read_bytes()
        for field in staging.FIELDS:
            path = staging.bundle_path(kind, field)
            assert (tmp_path / "fetched" / path).read_bytes() == (root / path).read_bytes()
    (tmp_path / "staged/files/withdraw-verification-key.json").write_text('{"swapped": true}')
    with pytest.raises(ValueError, match="verification key differs"):
        staging.fetch(manifest, record["hosting_manifest_sha256"], tmp_path / "fetched-2", serve_staged(tmp_path / "staged"))
    (tmp_path / "staged/files/transfer.zkey").write_bytes(b"transfer-zkeX")
    with pytest.raises(ValueError, match="differs from the manifest"):
        staging.fetch(manifest, record["hosting_manifest_sha256"], tmp_path / "fetched-3", serve_staged(tmp_path / "staged"))
    with pytest.raises(ValueError, match="pin"):
        staging.fetch(manifest, "0" * 64, tmp_path / "fetched-4", serve_staged(tmp_path / "staged"))


def recorded_domain(name, manifest_sha256, runtime=PIN, test_only=True):
    return {"manifest_sha256": manifest_sha256, "chain_id": 10143, "settlement_contract": POOLS[name],
            "verifier_version": 2, "test_only": test_only,
            "circuits": {kind: {"identity": {"verifier_runtime_keccak256": runtime}} for kind in staging.KINDS}}


def test_coverage_requires_exactly_one_matching_manifest_per_deployed_pool(tmp_path):
    record = deployment_record(tmp_path)
    both = {"proving_domains": [recorded_domain("wmon", "a" * 64), recorded_domain("usdc", "b" * 64)]}
    assert staging.deployment_coverage(record, both)["complete"]
    only = staging.deployment_coverage(record, {"proving_domains": both["proving_domains"][:1]})
    assert not only["complete"] and "usdc: packages recorded 0 manifests" in " ".join(only["problems"])
    drift = staging.deployment_coverage(record, {"proving_domains": [recorded_domain("wmon", "a" * 64, "0" * 63 + "2"),
                                                                       recorded_domain("usdc", "b" * 64, test_only=False)]})
    problems = " ".join(drift["problems"])
    assert "wmon: recorded transfer verifier runtime" in problems and "usdc: a test-only deployment" in problems
    extra = dict(recorded_domain("wmon", "c" * 64), settlement_contract="0x" + "99" * 20)
    outside = staging.deployment_coverage(record, {"proving_domains": both["proving_domains"] + [extra]})
    assert "outside the deployment" in " ".join(outside["problems"])
    assert not staging.deployment_coverage(record, {"proving_artifacts": None})["complete"]


def test_install_check_refuses_packages_missing_a_deployed_pool_and_misaligned_witnesses(tmp_path):
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "release.json").write_text(json.dumps({"channel": "metropolis-testnet", "published": False, "wheels": {},
                                                       "proving_domains": [recorded_domain("wmon", "a" * 64)]}))
    with pytest.raises(ValueError, match="do not cover the deployment"):
        checker.check(registry, require_deployment=deployment_record(tmp_path))
    with pytest.raises(ValueError, match="one --hosted-witnesses directory per hosted domain"):
        checker.check(registry, hosted_fixture=True, proving_witnesses=tmp_path,
                      hosted_fixture_bundles=[tmp_path / "a", tmp_path / "b"], hosted_witnesses=[tmp_path])
