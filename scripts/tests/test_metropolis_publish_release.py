"""Publication requires both native platforms, hosted proving for every authorized pool, and every rehearsal."""

import copy
import hashlib
import importlib.util
import json
import zipfile
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("metropolis_publish", ROOT / "scripts/publish-metropolis-release.py")
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)

PINS = {"wmon": "3" * 64, "usdc": "b" * 64}
PUBLISHER = "0x" + "cd" * 20
AUTHORIZATION = {"repository": publish.REPOSITORY, "package_version": "0.3.0.dev2026101202", "publisher": PUBLISHER,
                 "hosting_manifests": PINS, "artifact_publication_authorized": True, "qualification_publish": False,
                 "test_only": True, "single_operator": True, "audited": False, "mainnet_authorized": False}
HEADERS = {"macho-arm64": b"\xcf\xfa\xed\xfe" + (0x0100000C).to_bytes(4, "little"),
           "elf-x86_64": b"\x7fELF\x02\x01" + bytes(12) + (62).to_bytes(2, "little")}


def wheel(path: Path, tag: str, files: dict[str, bytes], licenses: tuple[str, ...]) -> str:
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("pkg.dist-info/WHEEL", f"Wheel-Version: 1.0\nTag: py3-none-{tag}\n")
        for name in licenses:
            archive.writestr(f"pkg.dist-info/licenses/{name}", "license")
        for name, data in files.items():
            archive.writestr(f"pkg.data/scripts/{name}", data)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def platform_dir(artifacts: Path, platform: str, binaries: set[str]) -> Path:
    tag, kind = publish.PLATFORMS[platform]
    root = artifacts / f"metropolis-registry-{platform}"
    (root / "wheels").mkdir(parents=True)
    (root / "qualification").mkdir()
    blobs = {name: HEADERS[kind] + name.encode() for name in sorted(binaries)}
    blobs["erebus-selfhost"] = b"#!/bin/sh\n"
    native = f"erebus_cli-0.3.0.dev2026101202-py3-none-{tag}.whl"
    pure = "erebus_sdk-0.3.0.dev2026101202-py3-none-any.whl"
    wheels = {native: wheel(root / "wheels" / native, tag, blobs, ("LICENSE", "THIRD_PARTY_NOTICES"))}
    shared = artifacts / pure
    if not shared.exists():
        wheel(shared, "any", {}, ("LICENSE",))
    (root / "wheels" / pure).write_bytes(shared.read_bytes())
    wheels[pure] = hashlib.sha256(shared.read_bytes()).hexdigest()
    release = {"version": AUTHORIZATION["package_version"], "platform": platform, "source_commit": "5" * 40,
               "dirty_source": False, "published": False, "test_only_artifacts_included": False, "build_profile": "release",
               "platform_qualification": {"wheel_tag": tag}, "licenses": {"project": "Apache-2.0"},
               "candidate": {"release_eligible": True, "source_tree_sha256": "e" * 64},
               "binaries": {name: {"sha256": hashlib.sha256(blob).hexdigest()} for name, blob in blobs.items() if name in binaries},
               "launchers": {"erebus-selfhost": {"sha256": hashlib.sha256(blobs["erebus-selfhost"]).hexdigest()}},
               "wheels": {name: {"sha256": digest} for name, digest in wheels.items()},
               "proving_domains": [{"manifest_sha256": pin} for pin in PINS.values()],
               "deployment_coverage": {"complete": True}}
    (root / "release.json").write_text(json.dumps(release))
    release_sha256 = hashlib.sha256((root / "release.json").read_bytes()).hexdigest()
    (root / "proving-mode").write_text("hosted\n")
    claims = {"native_platform_qualification": True, "live_payment": False}
    base = {"status": "verified", "source_imports": False, "release_json_sha256": release_sha256, "claims": claims}
    hosted = [{"manifest_sha256": pin, "settlement_contract": name, "proved": True, "https_exercised": True,
               "release_recorded": True, "publisher": PUBLISHER, "cache_tamper": "redownloaded",
               "authorization_refusals": {"wrong_publisher": True, "changed_manifest_bytes": True,
                                          "foreign_domain_public_inputs": True}} for name, pin in PINS.items()]
    reports = {"install": {**base, "installed_onboarding": {}, "installed_hosted_artifacts": hosted,
                           "deployment_coverage": {"complete": True, "pools": [{"pool": n, "proved": True} for n in PINS]}}}
    for gate, flag in publish.REHEARSALS.items():
        reports[gate] = {**base, flag: True}
    bound = {**base, "source_commit": release["source_commit"]}
    digests = {name: entry["sha256"] for name, entry in release["binaries"].items()}
    deal = {"payment_verified_by_recovery": True, "settlement_broadcasts": 1, "delivery": {"resource_verified": True},
            "auditor": {"payment_verified": True}}
    reports["workflow"] = {**bound, "pins_mode": "prepublication",
                           "installed_binaries_sha256": {n: digests[n] for n in ("erebus-negotiate", "erebus-payment", "erebus-wallet")
                                                         if n in digests} or digests,
                           "pools": {name: {"main": {"deal": deal}} for name in PINS}}
    reports["relayer"] = {**bound, "service": "installed-binary", "in_process": False,
                          "installed_binaries_sha256": {n: digests[n] for n in ("erebus-shielded-relayer",) if n in digests} or digests,
                          "gate_complete": True, "coverage_gaps": [],
                          "checks": {check: {"passed": True, "scope": f"{check} scope", "test": f"{check}_test"}
                                     for check in publish.RELAYER_CHECKS}}
    for gate, report in reports.items():
        (root / "qualification" / f"{gate}.json").write_text(json.dumps(report))
    for name, pin in PINS.items():
        (root / "qualification" / f"fetch-{name}.json").write_text(json.dumps({"hosted": True, "manifest_sha256": pin}))
    return root


@pytest.fixture
def artifacts(tmp_path):
    binaries = publish.expected_binaries()
    for platform in publish.PLATFORMS:
        platform_dir(tmp_path, platform, binaries)
    return tmp_path


def edit(path: Path, change):
    data = json.loads(path.read_text())
    change(data)
    path.write_text(json.dumps(data))


def problems(artifacts):
    return " ".join(publish.qualify(artifacts, AUTHORIZATION)[2])


def test_fully_qualified_artifacts_pass_and_keep_their_labels(artifacts):
    combined, assets, found = publish.qualify(artifacts, AUTHORIZATION)
    assert found == [] and combined["labels"] == ["test-only", "single-operator", "unaudited"]
    assert len(assets) == 3 and publish.check_authorization(AUTHORIZATION) == []


def test_binary_inventory_comes_from_the_builder_not_a_fixed_count(artifacts):
    assert {"erebus-shielded-disclosure", "erebus-shielded-relayer"} <= publish.expected_binaries()
    assert len(publish.expected_binaries()) == 16
    edit(artifacts / "metropolis-registry-linux-x86_64/release.json", lambda r: r["binaries"].pop("erebus-relay"))
    assert "not the build inventory" in problems(artifacts)


@pytest.mark.parametrize("gate,change,message", [
    ("install", lambda r: r["claims"].update(native_platform_qualification=False), "native hosted runner"),
    ("install", lambda r: r["installed_hosted_artifacts"][1].update(https_exercised=False), "real HTTPS"),
    ("install", lambda r: r["installed_hosted_artifacts"][0].update(publisher="0x" + "11" * 20), "different publisher"),
    ("install", lambda r: r["installed_hosted_artifacts"].pop(), "exactly the authorized manifests"),
    ("install", lambda r: r["installed_hosted_artifacts"][0]["authorization_refusals"].update(wrong_publisher=False), "refusals incomplete"),
    ("install", lambda r: r["deployment_coverage"]["pools"][1].update(proved=None), "every deployed pool"),
    ("install", lambda r: r.update(installed_onboarding=None), "onboarding"),
    ("metered", lambda r: r.update(installed_metered_rehearsal=False), "metered rehearsal did not run"),
    ("x402", lambda r: r.update(status="failed"), "x402 qualification did not verify"),
    ("assets", lambda r: r.update(release_json_sha256="0" * 64), "different release.json"),
    ("assets", lambda r: r["claims"].update(live_payment=True), "claims a live payment"),
    ("assets", lambda r: r.update(publication_evidence=False), "never publication evidence"),
    ("install", lambda r: r.update(rehearsal=True), "never publication evidence"),
    ("x402", lambda r: r.update(labels=["test-only", "rehearsal-pins"]), "never publication evidence"),
])
def test_each_missing_gate_blocks_publication(artifacts, gate, change, message):
    edit(artifacts / f"metropolis-registry-macos-arm64/qualification/{gate}.json", change)
    assert message in problems(artifacts)


def test_fixture_mode_unrecorded_manifests_fetch_gaps_and_wrong_version_block(artifacts):
    root = artifacts / "metropolis-registry-linux-x86_64"
    (root / "proving-mode").write_text("ci-fixture\n")
    (root / "qualification/fetch-usdc.json").unlink()
    edit(root / "release.json", lambda r: r.update(version="0.3.0.dev1", proving_domains=[{"manifest_sha256": "f" * 64}]))
    found = problems(artifacts)
    for message in ("not qualified against hosted artifacts", "not the authorized hosting manifests",
                    "HTTPS fetch reports do not cover", "not the authorized version", "platforms disagree"):
        assert message in found


def test_existing_guards_still_hold(artifacts):
    root = artifacts / "metropolis-registry-macos-arm64"
    edit(root / "release.json", lambda r: r.update(dirty_source=True))
    edit(artifacts / "metropolis-registry-linux-x86_64/release.json",
         lambda r: r["wheels"]["erebus_sdk-0.3.0.dev2026101202-py3-none-any.whl"].update(sha256="0" * 64))
    found = problems(artifacts)
    assert "dirty" in found and "hash does not match" in found


def test_authorization_must_keep_labels_and_pin_publisher_and_manifests():
    for change, message in ((dict(audited=True), "audited=false"), (dict(single_operator=False), "single_operator=true"),
                            (dict(publisher="0x" + "0" * 40), "publisher"), (dict(hosting_manifests={}), "hosting manifest"),
                            (dict(artifact_publication_authorized=False), "does not authorize"),
                            (dict(qualification_publish=True), "publish=false"), (dict(mainnet_authorized=True), "testnet")):
        assert message in " ".join(publish.check_authorization({**AUTHORIZATION, **change}))


def test_the_named_run_must_be_the_successful_qualification_of_this_commit():
    good = {"workflowName": "Metropolis registry", "event": "workflow_dispatch", "conclusion": "success", "headSha": "5" * 40,
            "jobs": [{"name": n, "conclusion": "success"} for n in ("macos-arm64", "linux-x86_64", "Compare platform builds")]
            + [{"name": "Publish registry", "conclusion": "skipped"}]}
    assert publish.check_run("1", "5" * 40, lambda: good) == []
    for change, message in ((dict(conclusion="failure"), "concluded failure"), (dict(headSha="6" * 40), "different commit"),
                            (dict(event="push"), "dispatched")):
        assert message in " ".join(publish.check_run("1", "5" * 40, lambda: {**good, **change}))
    failed = copy.deepcopy(good)
    failed["jobs"][1]["conclusion"] = "failure"
    failed["jobs"][3]["conclusion"] = "success"
    found = " ".join(publish.check_run("1", "5" * 40, lambda: failed))
    assert "linux-x86_64 did not succeed" in found and "must not have published" in found


@pytest.mark.parametrize("gate,change,message", [
    ("workflow", None, "missing workflow report"),
    ("relayer", None, "missing relayer report"),
    ("workflow", lambda r: r.update(release_json_sha256="0" * 64), "stale or for other packages"),
    ("relayer", lambda r: r.update(source_commit="6" * 40), "stale or for other packages"),
    ("workflow", lambda r: r.update(pins_mode="rehearsal"), "not pinned to this authorized version"),
    ("workflow", lambda r: r.update(rehearsal=True, publication_evidence=False), "never publication evidence"),
    ("workflow", lambda r: r["claims"].update(native_platform_qualification=False), "native hosted runner"),
    ("workflow", lambda r: r["pools"].pop("usdc"), "exactly the authorized pools"),
    ("workflow", lambda r: r["pools"]["wmon"]["main"]["deal"].update(settlement_broadcasts=2), "payment, delivery and audit"),
    ("workflow", lambda r: r["installed_binaries_sha256"].update({"erebus-wallet": "0" * 64}), "bind the installed binaries"),
    ("relayer", lambda r: r.update(service="in-process", in_process=True), "did not drive the installed binary"),
    ("relayer", lambda r: r["checks"]["no_direct_fallback"].update(passed=False), "missing no_direct_fallback"),
    ("relayer", lambda r: r["checks"].update(no_direct_fallback=True), "missing no_direct_fallback"),
    ("relayer", lambda r: r["checks"]["submit"].update(scope=""), "missing submit"),
    ("relayer", lambda r: r["checks"].pop("installed_payment_delegation"), "missing installed_payment_delegation"),
    ("relayer", lambda r: r["checks"]["installed_payment_delegation"].update(
        test="buyer_transport_delegates_to_the_installed_binary_and_never_signs_directly"), "only at the transport level"),
    ("relayer", lambda r: r.update(gate_complete=False, coverage_gaps=["installed_payment_delegation"]), "coverage gaps"),
    ("relayer", lambda r: r.update(release_json_sha256="0" * 64), "stale or for other packages"),
    ("relayer", lambda r: r["installed_binaries_sha256"].update({"erebus-shielded-relayer": "0" * 64}), "bind the installed binaries"),
    ("relayer", lambda r: r.update(status="failed"), "relayer report did not verify"),
])
def test_feature_gates_must_be_present_native_and_bound_to_these_packages(artifacts, gate, change, message):
    path = artifacts / f"metropolis-registry-linux-x86_64/qualification/{gate}.json"
    if change is None:
        path.unlink()
    else:
        edit(path, change)
    assert message in problems(artifacts)
