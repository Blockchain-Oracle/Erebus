"""A dirty build names its exact inputs and can never pass as a publishable release."""

import hashlib
import importlib.util
import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


registry = load("metropolis_candidate_registry", "scripts/build-metropolis-registry.py")
checker = load("metropolis_candidate_checker", "scripts/check-metropolis-install.py")


@pytest.fixture
def repository(tmp_path, monkeypatch):
    def git(*arguments):
        subprocess.run(["git", *arguments], cwd=tmp_path, check=True, capture_output=True)

    git("init", "-q")
    git("config", "user.email", "test@example.invalid")
    git("config", "user.name", "test")
    (tmp_path / ".gitignore").write_text("target/\n")
    for relative, text in {"sdk/shielded/src/lib.rs": "a", "sdk/shielded/tests/case.rs": "t",
                           "LICENSE": "l", "docs/notes.md": "d"}.items():
        (tmp_path / relative).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / relative).write_text(text)
    git("add", "-A")
    git("commit", "-qm", "base")
    monkeypatch.setattr(registry, "ROOT", tmp_path)
    return tmp_path, git


def test_clean_committed_tree_is_release_eligible(repository):
    identity = registry.candidate_identity()
    assert identity["dirty_source"] is False and identity["merge_head"] is None
    assert identity["release_eligible"] is True and identity["evidence_scope"] == "release-candidate"
    assert identity["changed_inputs"] == []
    # Recomputable from file contents alone: sorted `<path>\0<sha256>\n` lines.
    lines = "".join(f"{path}\0{hashlib.sha256(text.encode()).hexdigest()}\n"
                    for path, text in sorted({"LICENSE": "l", "sdk/shielded/src/lib.rs": "a"}.items()))
    assert identity["source_tree_sha256"] == hashlib.sha256(lines.encode()).hexdigest()
    assert identity["source_tree_files"] == 2


def test_uncommitted_and_untracked_inputs_change_the_identity(repository):
    root, _ = repository
    before = registry.candidate_identity()
    (root / "sdk/shielded/src/lib.rs").write_text("b")
    (root / "sdk/shielded/src/new.rs").write_text("n")
    after = registry.candidate_identity()
    assert after["source_tree_sha256"] != before["source_tree_sha256"]
    assert after["dirty_source"] is True and after["release_eligible"] is False
    assert after["evidence_scope"] == "team-rehearsal"
    changed = {entry["path"]: entry for entry in after["changed_inputs"]}
    assert changed["sdk/shielded/src/lib.rs"]["sha256"] == hashlib.sha256(b"b").hexdigest()
    assert changed["sdk/shielded/src/new.rs"]["status"] == "??"
    with pytest.raises(ValueError, match="changed during the build"):
        registry.require_same_candidate(before, after)


def test_test_and_doc_edits_do_not_change_shipped_inputs_but_block_release(repository):
    root, _ = repository
    before = registry.candidate_identity()
    (root / "sdk/shielded/tests/case.rs").write_text("other agent edit")
    (root / "docs/notes.md").write_text("doc edit")
    (root / "sdk/shielded/target").mkdir()
    (root / "sdk/shielded/target/out").write_text("ignored")
    after = registry.candidate_identity()
    registry.require_same_candidate(before, after)
    assert after["changed_inputs"] == []
    assert after["dirty_source"] is True and after["release_eligible"] is False


def test_deleted_tracked_input_is_recorded_without_a_digest(repository):
    root, _ = repository
    (root / "sdk/shielded/src/lib.rs").unlink()
    identity = registry.candidate_identity()
    assert identity["changed_inputs"] == [{"path": "sdk/shielded/src/lib.rs", "status": "D", "sha256": None}]


def proving_manifest(tmp_path, test_only=True):
    files = {kind: {"url": "https://example.invalid/x", "sha256": "1" * 64, "bytes": 1} for kind in ("wasm", "r1cs", "zkey")}
    body = {"version": 2, "chain_id": 10143, "settlement_contract": "0x" + "12" * 20, "verifier_version": 2,
            "test_only": test_only, "circuits": [{"kind": "transfer", **files}]}
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(body))
    return path, hashlib.sha256(path.read_bytes()).hexdigest()


def test_proving_manifest_is_recorded_only_against_its_pinned_digest(tmp_path):
    path, digest = proving_manifest(tmp_path)
    record = registry.proving_artifacts(path, digest, None)
    assert record["manifest_sha256"] == digest and record["test_only"] is True
    assert record["bundled_in_wheels"] is False
    assert record["circuits"]["transfer"]["zkey"] == {"sha256": "1" * 64, "bytes": 1}
    with pytest.raises(ValueError, match="identity mismatch"):
        registry.proving_artifacts(path, "0" * 64, None)
    with pytest.raises(ValueError, match="lowercase hex"):
        registry.proving_artifacts(path, digest.upper(), None)


def test_non_test_proving_manifest_requires_a_signature(tmp_path):
    path, digest = proving_manifest(tmp_path, test_only=False)
    with pytest.raises(ValueError, match="signature"):
        registry.proving_artifacts(path, digest, None)
    signature = tmp_path / "signature.json"
    signature.write_text("{}")
    assert registry.proving_artifacts(path, digest, signature)["signature_sha256"] == hashlib.sha256(b"{}").hexdigest()


def test_install_report_is_always_team_rehearsal_evidence():
    clean = {"source_commit": "a" * 40, "dirty_source": False, "candidate": {"release_eligible": True}}
    report = checker.evidence(clean)
    assert report["evidence_scope"] == "team-rehearsal" and report["outside_acceptance"] is False
    assert report["release_eligible"] is True
    assert checker.evidence({"dirty_source": True})["release_eligible"] is False


def test_prover_request_must_name_the_recorded_proving_manifest(tmp_path):
    request = tmp_path / "request.json"
    request.write_text(json.dumps({"manifest_sha256": "a" * 64}))
    checker.require_recorded_proving_manifest({}, request)
    checker.require_recorded_proving_manifest({"proving_artifacts": {"manifest_sha256": "a" * 64}}, request)
    with pytest.raises(ValueError, match="different proving manifest"):
        checker.require_recorded_proving_manifest({"proving_artifacts": {"manifest_sha256": "b" * 64}}, request)


@pytest.mark.parametrize("recorded,test_only,message", [
    (None, True, "release recorded"),
    ("0" * 64, True, "release recorded"),
    ("match", False, "test-only"),
])
def test_cold_proving_refuses_unrecorded_or_release_bundles_before_serving(tmp_path, recorded, test_only, message):
    bundle = tmp_path / "bundle"
    bundle.mkdir()
    (bundle / "manifest.json").write_text(json.dumps({"test_only": test_only, "circuits": []}))
    digest = hashlib.sha256((bundle / "manifest.json").read_bytes()).hexdigest()
    manifest = {} if recorded is None else {"proving_artifacts": {"manifest_sha256": digest if recorded == "match" else recorded}}
    root = tmp_path / "root"
    root.mkdir()
    with pytest.raises(ValueError, match=message):
        checker.rehearse_cold_proving(tmp_path / "bin", manifest, bundle, tmp_path, {}, root, 60)
    assert list(root.iterdir()) == []


def test_cold_proving_needs_both_bundle_and_witnesses(tmp_path):
    with pytest.raises(ValueError, match="both a bundle and witness"):
        checker.check(tmp_path, proving_bundle=tmp_path)


def test_packages_record_one_proving_manifest_per_distinct_pool_domain(tmp_path):
    first = {"manifest_sha256": "a" * 64, "chain_id": 10143, "settlement_contract": "0x" + "88" * 20, "test_only": True}
    second = dict(first, manifest_sha256="b" * 64, settlement_contract="0x" + "86" * 20)
    assert registry.proving_domains([first, second]) == [first, second]
    with pytest.raises(ValueError, match="distinct pool domain"):
        registry.proving_domains([first, dict(second, settlement_contract=first["settlement_contract"].upper())])
    with pytest.raises(ValueError, match="distinct pool domain"):
        registry.proving_domains([first, dict(first)])
    with pytest.raises(ValueError, match="fixture boundary"):
        registry.proving_domains([first, dict(second, test_only=False)])
    request = tmp_path / "request.json"
    request.write_text(json.dumps({"manifest_sha256": "b" * 64}))
    checker.require_recorded_proving_manifest({"proving_artifacts": None, "proving_domains": [first, second]}, request)
    with pytest.raises(ValueError, match="different proving manifest"):
        checker.require_recorded_proving_manifest({"proving_domains": [first]}, request)


def test_builder_refuses_packages_that_omit_a_deployed_pool(tmp_path):
    pin = "0" * 63 + "1"
    record = tmp_path / "deployment.json"
    record.write_text(json.dumps({"chain_id": 10143, "test_only": True, "pools": [
        {"name": name, "pool": pool, "namespace": "eip155:10143", "verifier_version": 2,
         "runtime_pins": {key: pin for key in ("pool", "poseidon", "deposit", "transfer", "withdraw")}}
        for name, pool in (("wmon", "0x" + "88" * 20), ("usdc", "0x" + "86" * 20))]}))
    domain = lambda pool, digest: {"manifest_sha256": digest, "chain_id": 10143, "settlement_contract": pool,
                                   "verifier_version": 2, "test_only": True,
                                   "circuits": {k: {"identity": {"verifier_runtime_keccak256": pin}}
                                                for k in ("deposit", "transfer", "withdraw")}}
    wmon, usdc = domain("0x" + "88" * 20, "a" * 64), domain("0x" + "86" * 20, "b" * 64)
    assert registry.require_deployment(record, [wmon, usdc])["complete"]
    with pytest.raises(ValueError, match="usdc: packages recorded 0 manifests"):
        registry.require_deployment(record, [wmon])
