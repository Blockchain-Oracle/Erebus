"""The installed-relayer report adapter binds results to the packages and never overstates a test's scope."""

import hashlib
import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def load(name: str, relative: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


adapter = load("metropolis_relayer_report", "scripts/qualify-metropolis-relayer.py")
publish = load("metropolis_publish_for_relayer", "scripts/publish-metropolis-release.py")
RELAYER = b"\xcf\xfa\xed\xfe packaged relayer"
PAYMENT = b"\xcf\xfa\xed\xfe packaged payment"


@pytest.fixture
def world(tmp_path, monkeypatch):
    monkeypatch.delenv("GITHUB_ACTIONS", raising=False)
    monkeypatch.delenv("RUNNER_ENVIRONMENT", raising=False)
    registry = tmp_path / "registry"
    registry.mkdir()
    release = {"version": "0.3.0.dev2026101306", "source_commit": "5" * 40, "dirty_source": False,
               "binaries": {"erebus-shielded-relayer": {"sha256": hashlib.sha256(RELAYER).hexdigest()},
                            "erebus-payment": {"sha256": hashlib.sha256(PAYMENT).hexdigest()}},
               "candidate": {"source_tree_sha256": "a" * 64}}
    (registry / "release.json").write_text(json.dumps(release))
    checkout = tmp_path / "checkout"
    for name in adapter.FIXTURES:
        (checkout / name).parent.mkdir(parents=True, exist_ok=True)
        (checkout / name).write_text(name)

    def install(work, version, registry_dir, index, payload=RELAYER, payment=PAYMENT):
        bins = work / "env/bin"
        bins.mkdir(parents=True)
        (bins / "erebus-shielded-relayer").write_bytes(payload)
        (bins / "erebus-payment").write_bytes(payment)
        return bins

    return tmp_path, registry, checkout, install


def passing(checkout, binaries, arguments, log):
    names = ([name for name in (*adapter.DELEGATION_TESTS, *adapter.MCP_TESTS) if name in arguments] or [adapter.GATES]) if "--exact" in arguments else adapter.FUNDED
    return {name: "ok" for name in names}


def test_a_passing_fixture_run_is_bound_and_scoped_but_not_a_complete_gate(world, monkeypatch):
    monkeypatch.setattr(adapter, "DELEGATION_TESTS", ())
    root, registry, checkout, install = world
    seen = []

    def run(checkout_dir, binaries, arguments, log):
        seen.append((binaries, arguments))
        return passing(checkout_dir, binaries, arguments, log)

    report = adapter.qualify(registry, "0.3.0.dev2026101306", root / "work", checkout, run=run, install=install)
    assert all(binaries == {"erebus-shielded-relayer": root / "work/env/bin/erebus-shielded-relayer",
                            "erebus-payment": root / "work/env/bin/erebus-payment"} for binaries, _ in seen), "explicit pins"
    assert report["status"] == "verified" and report["service"] == "installed-binary" and report["in_process"] is False
    assert report["release_json_sha256"] == hashlib.sha256((registry / "release.json").read_bytes()).hexdigest()
    assert report["source_commit"] == "5" * 40
    assert report["installed_binaries_sha256"] == {"erebus-shielded-relayer": hashlib.sha256(RELAYER).hexdigest(),
                                                   "erebus-payment": hashlib.sha256(PAYMENT).hexdigest()}
    assert set(report["fixture_files_sha256"]) == set(adapter.FIXTURES)
    # The buyer test is transport-only: it covers no_direct_fallback at that scope and never payment delegation.
    assert "NOT the installed erebus-payment CLI or MCP" in report["checks"]["no_direct_fallback"]["scope"]
    assert report["checks"]["installed_payment_delegation"] == {
        "passed": False, "test": None, "scope": report["checks"]["installed_payment_delegation"]["scope"]}
    assert report["gate_complete"] is False and report["coverage_gaps"] == ["installed_payment_delegation"]
    assert report["claims"] == {**report["claims"], "native_platform_qualification": False, "live_payment": False}


def test_publication_refuses_the_adapter_report_until_installed_delegation_is_covered(world, monkeypatch):
    monkeypatch.setattr(adapter, "DELEGATION_TESTS", ())
    root, registry, checkout, install = world
    monkeypatch.setenv("GITHUB_ACTIONS", "true")
    monkeypatch.setenv("RUNNER_ENVIRONMENT", "github-hosted")
    report = adapter.qualify(registry, "0.3.0.dev2026101306", root / "work", checkout, run=passing, install=install)
    assert report["claims"]["native_platform_qualification"] is True
    platform = root / "metropolis-registry-macos-arm64"
    (platform / "qualification").mkdir(parents=True)
    (platform / "qualification/relayer.json").write_text(json.dumps(report))
    manifest = json.loads((registry / "release.json").read_text())
    release_sha256 = report["release_json_sha256"]
    _, problems = publish.bound_report("macos-arm64", platform, "relayer", manifest, release_sha256)
    assert problems == [], "identity binding, native claim and status hold"
    found = " ".join(publish.check_feature_gates("macos-arm64", platform, manifest, release_sha256, {}))
    assert "missing installed_payment_delegation" in found and "coverage gaps" in found
    assert "missing no_direct_fallback" not in found and "missing submit" not in found


@pytest.mark.parametrize("outcome", ["FAILED", "not run"])
def test_a_failed_or_missing_test_fails_the_report(world, outcome):
    root, registry, checkout, install = world

    def run(checkout_dir, binaries, arguments, log):
        results = passing(checkout_dir, binaries, arguments, log)
        if adapter.DUPLICATE in results:
            if outcome == "not run":
                del results[adapter.DUPLICATE]
            else:
                results[adapter.DUPLICATE] = outcome
        return results

    report = adapter.qualify(registry, "v", root / "work", checkout, run=run, install=install)
    assert report["status"] == "failed" and report["checks"]["duplicate_one_broadcast"]["passed"] is False


def test_a_relayer_that_is_not_the_packaged_binary_is_refused_before_any_test(world):
    root, registry, checkout, install = world
    ran = []
    with pytest.raises(ValueError, match="not the packaged binary"):
        adapter.qualify(registry, "v", root / "work", checkout, run=lambda *a: ran.append(a) or {},
                        install=lambda *a: install(*a, payload=b"cargo-built copy"))
    assert ran == []


def test_packages_without_a_relayer_are_refused(world):
    root, registry, checkout, install = world
    release = json.loads((registry / "release.json").read_text())
    release["binaries"] = {}
    (registry / "release.json").write_text(json.dumps(release))
    with pytest.raises(ValueError, match="do not include erebus-shielded-relayer"):
        adapter.qualify(registry, "v", root / "work", checkout, run=passing, install=install)


def test_the_adapter_never_sets_cargo_built_binaries_or_inherits_erebus_settings(monkeypatch, tmp_path):
    captured = {}
    monkeypatch.setattr(adapter.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setenv("EREBUS_TEST_SHIELDED_RELAYER", "/somewhere/else")
    monkeypatch.setenv("EREBUS_BACKEND", "mock")

    def fake(command, **kwargs):
        captured.update(command=command, env=kwargs["env"])
        return type("R", (), {"stdout": f"test {adapter.SUBMIT} ... ok\n", "stderr": ""})()

    monkeypatch.setattr(adapter.subprocess, "run", fake)
    pinned = {"erebus-shielded-relayer": tmp_path / "installed/erebus-shielded-relayer",
              "erebus-payment": tmp_path / "installed/erebus-payment"}
    results = adapter.cargo_tests(tmp_path, pinned, ["--ignored"], tmp_path / "log")
    assert results == {adapter.SUBMIT: "ok"}
    assert captured["env"]["EREBUS_TEST_SHIELDED_RELAYER"] == str(pinned["erebus-shielded-relayer"])
    assert captured["env"]["EREBUS_TEST_PAYMENT"] == str(pinned["erebus-payment"])
    assert "EREBUS_BACKEND" not in captured["env"] and "--release" in captured["command"]


def test_hosted_ci_writes_the_relayer_report_from_the_built_registry():
    workflow = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    step = workflow[workflow.index("Native installed relayer fixture"):]
    step = step[:step.index("- uses: actions/upload-artifact")]
    assert "if: ${{ steps.proving.outputs.mode == 'hosted' }}" in step
    assert "scripts/qualify-metropolis-relayer.py --registry registry --version \"$METROPOLIS_VERSION\"" in step
    assert "> registry/qualification/relayer.json" in step
    assert "EREBUS_TEST_SHIELDED_RELAYER" not in step and "cargo build" not in step, "the adapter supplies the installed path"


def test_the_compare_job_checks_out_the_built_commit_before_loading_the_feature_gate_checker():
    workflow = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    compare = workflow[workflow.index("    name: Compare platform builds"):workflow.index("\n  publish:\n")]
    checkout = compare.index("uses: actions/checkout@v4")
    assert "ref: ${{ github.sha }}" in compare[checkout:checkout + 120]
    assert checkout < compare.index('"scripts/publish-metropolis-release.py"')
    assert checkout < compare.index('"packaging/metropolis/testnet-release.json"')


def test_a_payment_binary_that_is_not_the_packaged_one_is_refused(world):
    root, registry, checkout, install = world
    with pytest.raises(ValueError, match="erebus-payment is not the packaged binary"):
        adapter.qualify(registry, "v", root / "work", checkout, run=passing,
                        install=lambda *a: install(*a, payment=b"cargo-built payment"))


def test_a_named_delegation_test_must_pass_and_the_transport_test_can_never_be_one(world, monkeypatch):
    root, registry, checkout, install = world
    delegation = "installed_payment_cli_delegates_through_the_installed_relayer"
    monkeypatch.setattr(adapter, "DELEGATION_TESTS", (delegation,))
    report = adapter.qualify(registry, "v", root / "w1", checkout, run=passing, install=install)
    assert report["checks"]["installed_payment_delegation"]["passed"] is True and report["gate_complete"] is True

    def failing(checkout_dir, binaries, arguments, log):
        return {**passing(checkout_dir, binaries, arguments, log), delegation: "FAILED"}

    report = adapter.qualify(registry, "v", root / "w2", checkout, run=failing, install=install)
    assert report["status"] == "failed" and report["checks"]["installed_payment_delegation"]["passed"] is False
    monkeypatch.setattr(adapter, "DELEGATION_TESTS", (adapter.TRANSPORT,))
    with pytest.raises(ValueError, match="transport-only test cannot stand in"):
        adapter.qualify(registry, "v", root / "w3", checkout, run=passing, install=install)


def test_fixture_evidence_lines_are_kept_verbatim(tmp_path):
    log = tmp_path / "log"
    log.write_text('noise\nEREBUS_EVIDENCE {"test": "t", "buyer_sends": 0, "sponsor_sends": 1}\nEREBUS_EVIDENCE {bad\n')
    assert adapter.evidence_lines(log) == [{"test": "t", "buyer_sends": 0, "sponsor_sends": 1}]


def test_registered_installed_cli_delegation_is_not_a_transport_substitute(world):
    root, registry, checkout, install = world
    assert adapter.DELEGATION_TESTS == (
        "payment_driver::shielded_driver::installed_payment_delegation",
    )
    report = adapter.qualify(registry, "v", root / "work", checkout, run=passing, install=install)
    assert report["gate_complete"] is True
    assert report["checks"]["installed_payment_delegation"]["test"] not in publish.TRANSPORT_ONLY_TESTS
    assert "CLI scope, not MCP" in report["checks"]["installed_payment_delegation"]["scope"]

    def missing(checkout_dir, binaries, arguments, log):
        return {name: result for name, result in passing(checkout_dir, binaries, arguments, log).items()
                if name not in adapter.DELEGATION_TESTS}

    report = adapter.qualify(registry, "v", root / "missing", checkout, run=missing, install=install)
    assert report["gate_complete"] is False
    assert "installed_payment_delegation" in report["coverage_gaps"]


@pytest.mark.parametrize("changed", ["manifest", "binary", "fixture"])
def test_identity_drift_during_qualification_is_refused(world, changed):
    root, registry, checkout, install = world

    def drifting(checkout_dir, binaries, arguments, log):
        path = {"manifest": registry / "release.json",
                "binary": binaries["erebus-payment"],
                "fixture": checkout_dir / adapter.FIXTURES[0]}[changed]
        path.write_bytes(path.read_bytes() + b" ")
        return passing(checkout_dir, binaries, arguments, log)

    with pytest.raises(ValueError, match="changed during qualification"):
        adapter.qualify(registry, "v", root / "work", checkout, run=drifting, install=install)


def test_mcp_qualification_requires_its_own_test_and_rejects_cli_substitution(world):
    root, registry, checkout, install = world

    def missing(checkout_dir, binaries, arguments, log):
        return {name: result for name, result in passing(checkout_dir, binaries, arguments, log).items()
                if name not in adapter.MCP_TESTS}

    report = adapter.qualify(registry, "v", root / "missing", checkout, run=missing, install=install)
    assert report["gate_complete"] is False
    assert report["coverage_gaps"] == ["installed_mcp_delegation"]

    report = adapter.qualify(registry, "v", root / "passing", checkout, run=passing, install=install)
    report["checks"]["installed_mcp_delegation"]["test"] = report["checks"]["installed_payment_delegation"]["test"]
    platform = root / "metropolis-registry-macos-arm64"
    (platform / "qualification").mkdir(parents=True)
    (platform / "qualification/relayer.json").write_text(json.dumps(report))
    manifest = json.loads((registry / "release.json").read_text())
    problems = publish.check_feature_gates("macos-arm64", platform, manifest, report["release_json_sha256"], {})
    assert any("MCP sponsorship lacks its own installed tool test" in problem for problem in problems)
