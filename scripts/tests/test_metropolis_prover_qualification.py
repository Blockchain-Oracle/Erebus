import importlib.util
import json
import os
import shutil
import sys
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("prover_qualification", ROOT / "scripts/qualify-metropolis-prover.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def inputs(tmp_path):
    binary = tmp_path / "prover"
    binary.write_bytes(b"pinned-binary")
    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps({"test_only": True}))
    request = tmp_path / "request.json"
    request.write_text(json.dumps({"manifest_file": str(manifest), "manifest_sha256": module.digest(manifest), "witness_file": "SECRET-WITNESS-PATH"}))
    request.chmod(0o600)
    return binary, module.digest(binary), request


def test_qualification_reports_only_whitelisted_metrics(tmp_path, monkeypatch):
    response = {"status": "ok", "downloaded_bytes": 12, "cache_hits": 2, "installation_ms": 3, "proving_ms": 4, "calldata": "PRIVATE-OR-PROOF-DATA", "extra": "SECRET"}
    monkeypatch.setattr(module.subprocess, "run", lambda *args, **kwargs: subprocess.CompletedProcess(args, 0, json.dumps(response).encode(), b"SECRET-ERROR"))
    report = module.qualify(*inputs(tmp_path))
    assert report["verified"] is True
    assert report["test_only"] is True
    assert "SECRET" not in json.dumps(report)
    assert "PRIVATE" not in json.dumps(report)
    assert report["elapsed_ms"] >= 0
    assert report["child_peak_rss_bytes"] >= 0


@pytest.mark.parametrize("response", [{"status": "error", "error": "SECRET"}, {"status": "ok", "downloaded_bytes": True}, ["SECRET"], "SECRET"])
def test_failure_is_not_qualification_or_secret_output(tmp_path, monkeypatch, response):
    monkeypatch.setattr(module.subprocess, "run", lambda *args, **kwargs: subprocess.CompletedProcess(args, 1, json.dumps(response).encode(), b"SECRET"))
    report = module.qualify(*inputs(tmp_path))
    assert report["verified"] is False
    assert "SECRET" not in json.dumps(report)


def test_timeout_is_failure_without_partial_output(tmp_path, monkeypatch):
    def timeout(*args, **kwargs):
        raise subprocess.TimeoutExpired("prover", 1, output=b"SECRET")
    monkeypatch.setattr(module.subprocess, "run", timeout)
    report = module.qualify(*inputs(tmp_path))
    assert report["timed_out"] and not report["verified"]
    assert "SECRET" not in json.dumps(report)


@pytest.mark.parametrize("value", [-1, True, "1", None])
def test_success_status_with_invalid_metrics_is_rejected(tmp_path, monkeypatch, value):
    response = {"status": "ok", "downloaded_bytes": value, "cache_hits": 2, "installation_ms": 3, "proving_ms": 4}
    monkeypatch.setattr(module.subprocess, "run", lambda *args, **kwargs: subprocess.CompletedProcess(args, 0, json.dumps(response).encode(), b""))
    assert not module.qualify(*inputs(tmp_path))["verified"]


def test_binary_hash_and_request_permissions_fail_before_execution(tmp_path, monkeypatch):
    monkeypatch.setattr(module.subprocess, "run", lambda *args, **kwargs: pytest.fail("must not execute"))
    binary, hash_value, request = inputs(tmp_path)
    with pytest.raises(ValueError, match="identity"):
        module.qualify(binary, "00" * 32, request)
    request.chmod(0o644)
    with pytest.raises(ValueError, match="owner-only"):
        module.qualify(binary, hash_value, request)


def test_install_checker_qualifies_only_the_installed_hash_pinned_binary(tmp_path, monkeypatch):
    checker_spec = importlib.util.spec_from_file_location("metropolis_install_check", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    bins = tmp_path / "environment/bin"
    bins.mkdir(parents=True)
    binary, _, request = inputs(tmp_path)
    installed = bins / "erebus-local-prove"
    installed.write_bytes(binary.read_bytes())
    manifest = {"binaries": {"erebus-local-prove": {"sha256": module.digest(installed)}}}
    calls = []
    def run(command, **kwargs):
        calls.append((command, kwargs))
        response = {"verified": True, "source_build": False, "rss_scope": "prover_child_high_water"}
        return subprocess.CompletedProcess(command, 0, json.dumps(response), "")
    monkeypatch.setattr(subprocess, "run", run)
    report = checker.qualify_installed_prover(bins, manifest, request, {"PATH": str(bins)})
    assert report["verified"] and not report["source_build"]
    assert calls[0][0][0] == sys.executable
    assert calls[0][0][2:] == ["--binary", str(installed.resolve()), "--binary-sha256", module.digest(installed), "--request", str(request.resolve()), "--timeout", "600"]
    assert calls[0][1]["cwd"] == bins.parent
    assert calls[0][1]["env"] == {"PATH": str(bins)}
    assert calls[0][1]["timeout"] == 660
    checker.qualify_installed_prover(bins, manifest, request, {"PATH": str(bins)}, 3600)
    assert calls[1][0][-2:] == ["--timeout", "3600"]
    assert calls[1][1]["timeout"] == 3660
    installed.write_bytes(b"changed-installed-binary")
    with pytest.raises(ValueError, match="identity mismatch"):
        checker.qualify_installed_prover(bins, manifest, request, {})
    assert len(calls) == 2


@pytest.mark.parametrize("timeout", [0, 3601, True, 1.5, "600"])
def test_install_qualification_timeout_is_bounded_before_work(tmp_path, monkeypatch, timeout):
    checker_spec = importlib.util.spec_from_file_location("metropolis_timeout_bounds", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: pytest.fail("must not execute"))
    with pytest.raises(ValueError, match="qualification timeout"):
        checker.qualify_installed_prover(tmp_path, {}, tmp_path / "missing", {}, timeout)
    with pytest.raises(ValueError, match="qualification timeout"):
        checker.check(tmp_path, prover_timeout_seconds=timeout)


def test_install_checker_rejects_failed_proving(tmp_path, monkeypatch):
    checker_spec = importlib.util.spec_from_file_location("metropolis_install_check_failed", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    binary, _, request = inputs(tmp_path)
    installed = tmp_path / "erebus-local-prove"
    installed.write_bytes(binary.read_bytes())
    manifest = {"binaries": {"erebus-local-prove": {"sha256": module.digest(installed)}}}
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: subprocess.CompletedProcess(args, 1, '{"status":"error"}', "SECRET"))
    with pytest.raises(ValueError, match="qualification failed"):
        checker.qualify_installed_prover(tmp_path, manifest, request, {})


@pytest.mark.parametrize("report", [[], {"verified": True, "source_build": True, "rss_scope": "prover_child_high_water"},
    {"verified": True, "source_build": False, "rss_scope": "cumulative_child_high_water"}])
def test_install_checker_requires_a_fresh_runner_report(tmp_path, monkeypatch, report):
    checker_spec = importlib.util.spec_from_file_location("metropolis_fresh_runner_refusal", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    binary, _, request = inputs(tmp_path)
    installed = tmp_path / "erebus-local-prove"
    installed.write_bytes(binary.read_bytes())
    manifest = {"binaries": {"erebus-local-prove": {"sha256": module.digest(installed)}}}
    monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: subprocess.CompletedProcess(args, 0, json.dumps(report), ""))
    with pytest.raises(ValueError, match="qualification failed"):
        checker.qualify_installed_prover(tmp_path, manifest, request, {})


@pytest.mark.parametrize("prover_request,rehearse,metered,assets", [
    (None, False, False, False),
    (Path("request.json"), True, False, False),
    (Path("request.json"), False, True, False),
    (Path("request.json"), False, False, True),
])
def test_offline_install_refuses_ambiguous_scope_before_any_subprocess(tmp_path, monkeypatch, prover_request, rehearse, metered, assets):
    checker_spec = importlib.util.spec_from_file_location("metropolis_offline_scope", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    (tmp_path / "release.json").write_text(json.dumps({"channel": "metropolis-testnet", "published": False, "wheels": {}}))
    monkeypatch.setattr(checker.shutil, "which", lambda name: "/unused/uv")
    monkeypatch.setattr(checker.subprocess, "run", lambda *args, **kwargs: pytest.fail("must not execute"))
    with pytest.raises(ValueError, match="requires a prover request"):
        checker.check(tmp_path, rehearse_x402=rehearse, rehearse_metered=metered,
                      rehearse_assets=assets, prover_request=prover_request, offline_native_prover=True)


TRANSITIONS = [
    ("deposit", ["chainId", "contractAddress", "verifierVersion", "asset", "amount", "noteCommitment"]),
    ("transfer", ["chainId", "contractAddress", "verifierVersion", "asset", "dealCommitment", "dealNullifier", "root", "inputNullifier", "paymentCommitment", "changeCommitment", "expiry"]),
    ("withdraw", ["chainId", "contractAddress", "verifierVersion", "asset", "root", "noteNullifier", "recipient", "amount"]),
]


def qualification_binary(crate, name):
    directory = os.environ.get("EREBUS_M5_QUALIFY_BIN_DIR")
    return Path(directory) / name if directory else ROOT / crate / "target/debug" / name


def wheel_build_timeout():
    timeout = int(os.environ.get("EREBUS_M5_QUALIFY_WHEEL_BUILD_TIMEOUT_SECONDS", "180"))
    if not 1 <= timeout <= 3600:
        raise ValueError("wheel qualification build timeout must be between 1 and 3600 seconds")
    return timeout


def test_wheel_build_timeout_is_an_explicit_bounded_test_override(monkeypatch):
    monkeypatch.delenv("EREBUS_M5_QUALIFY_WHEEL_BUILD_TIMEOUT_SECONDS", raising=False)
    assert wheel_build_timeout() == 180
    monkeypatch.setenv("EREBUS_M5_QUALIFY_WHEEL_BUILD_TIMEOUT_SECONDS", "1800")
    assert wheel_build_timeout() == 1800
    for value in ["0", "3601", "invalid"]:
        monkeypatch.setenv("EREBUS_M5_QUALIFY_WHEEL_BUILD_TIMEOUT_SECONDS", value)
        with pytest.raises(ValueError):
            wheel_build_timeout()


def test_qualification_binary_directory_is_an_explicit_test_override(monkeypatch):
    monkeypatch.delenv("EREBUS_M5_QUALIFY_BIN_DIR", raising=False)
    assert qualification_binary("sdk/shielded", "erebus-artifacts") == ROOT / "sdk/shielded/target/debug/erebus-artifacts"
    monkeypatch.setenv("EREBUS_M5_QUALIFY_BIN_DIR", "/vm-build/debug")
    assert qualification_binary("sdk/shielded", "erebus-artifacts") == Path("/vm-build/debug/erebus-artifacts")


def fixture_request(tmp_path, kind, public_names, artifact_bundle=None):
    original_build = ROOT / "circuits/m5/build"
    build = artifact_bundle or original_build
    cache = tmp_path / "cache"
    cache.mkdir(mode=0o700)
    circuits = []
    for circuit in ["deposit", "transfer", "withdraw"]:
        record = {"kind": circuit}
        for field in ["wasm", "r1cs", "zkey"]:
            path = build / (f"{circuit}_js/{circuit}.wasm" if field == "wasm" else f"{circuit}.{field}")
            hash_value = module.digest(path)
            record[field] = {"url": f"https://fixtures.invalid/{circuit}/{field}", "bytes": path.stat().st_size, "sha256": hash_value}
            shutil.copyfile(path, cache / f"{hash_value}.artifact")
        circuits.append(record)
    witness = tmp_path / "witness.json"
    shutil.copyfile(original_build / f"{kind}-input.json", witness)
    witness.chmod(0o600)
    values = json.loads(witness.read_text())
    manifest = tmp_path / "manifest.json"
    if artifact_bundle is None:
        manifest.write_text(json.dumps({"version": 1, "chain_id": int(values["chainId"]), "settlement_contract": f'0x{int(values["contractAddress"]):040x}', "verifier_version": int(values["verifierVersion"]), "test_only": True, "circuits": circuits}))
    else:
        shutil.copyfile(artifact_bundle / "manifest.json", manifest)
    request = tmp_path / "request.json"
    request.write_text(json.dumps({"manifest_file": str(manifest), "manifest_sha256": module.digest(manifest), "circuit": kind, "cache_root": str(cache), "witness_file": str(witness), "expected_public": [values[name] for name in public_names], "allow_test_artifacts": True}))
    request.chmod(0o600)
    if artifact_bundle is not None:
        publisher_key = tmp_path / "publisher.key"
        publisher_key.write_bytes(os.urandom(32))
        publisher_key.chmod(0o600)
        try:
            signed = subprocess.run([str(qualification_binary("sdk/shielded", "erebus-artifacts")), "--sign-manifest"],
                input=json.dumps({"manifest_file": str(manifest), "publisher_key_file": str(publisher_key)}),
                text=True, capture_output=True, check=True, timeout=30)
            authorization = json.loads(signed.stdout)
            assert authorization["status"] == "ok" and not signed.stderr
            payload = json.loads(request.read_text())
            payload["manifest_authorization"] = {"publisher": authorization["publisher"], "signature": authorization["signature"]}
            request.write_text(json.dumps(payload))
        finally:
            publisher_key.unlink()
    return request, witness


def test_fixture_publisher_key_is_removed_when_signing_fails(tmp_path, monkeypatch):
    workspace = tmp_path / "workspace"
    build = workspace / "circuits/m5/build"
    build.mkdir(parents=True)
    for kind, names in TRANSITIONS:
        wasm = build / f"{kind}_js/{kind}.wasm"
        wasm.parent.mkdir()
        wasm.write_bytes(b"public-wasm")
        for suffix in ["r1cs", "zkey"]:
            (build / f"{kind}.{suffix}").write_bytes(b"public-artifact")
        (build / f"{kind}-input.json").write_text(json.dumps({name: "1" for name in names}))
    (build / "manifest.json").write_text('{"test_only":true}')
    case = tmp_path / "case"
    case.mkdir()
    monkeypatch.setattr(sys.modules[__name__], "ROOT", workspace)

    def fail_signing(*args, **kwargs):
        assert (case / "publisher.key").is_file()
        raise subprocess.CalledProcessError(2, args[0])

    monkeypatch.setattr(subprocess, "run", fail_signing)
    with pytest.raises(subprocess.CalledProcessError):
        fixture_request(case, *TRANSITIONS[0], build)
    assert not (case / "publisher.key").exists()


@pytest.fixture(scope="module")
def fixture_artifact_bundle(tmp_path_factory):
    supplied = os.environ.get("EREBUS_M5_QUALIFY_BUNDLE")
    if supplied:
        output = Path(supplied).resolve()
        manifest = json.loads((output / "manifest.json").read_text())
        assert manifest["version"] == 2 and manifest["test_only"] is True
        assert len(manifest["circuits"]) == 3
        for circuit in manifest["circuits"]:
            assert circuit["kind"] in {"deposit", "transfer", "withdraw"}
            for field in ["wasm", "r1cs", "zkey"]:
                path = output / (f'{circuit["kind"]}_js/{circuit["kind"]}.wasm' if field == "wasm" else f'{circuit["kind"]}.{field}')
                assert module.digest(path) == circuit[field]["sha256"]
                assert path.stat().st_size == circuit[field]["bytes"]
        return output
    root = tmp_path_factory.mktemp("signed-prover-artifacts")
    values = json.loads((ROOT / "circuits/m5/build/deposit-input.json").read_text())
    output = root / "bundle"
    plan = root / "plan.json"
    plan.write_text(json.dumps({"test_only": True, "build_dir": str(ROOT / "circuits/m5/build"),
        "output_dir": str(output), "base_url": "https://fixtures.invalid/bundle/",
        "chain_id": int(values["chainId"]), "settlement_contract": f'0x{int(values["contractAddress"]):040x}',
        "verifier_version": int(values["verifierVersion"])}))
    subprocess.run(["node", str(ROOT / "circuits/m5/scripts/build-artifact-bundle.mjs"), str(plan)], check=True, timeout=240)
    manifest = json.loads((output / "manifest.json").read_text())
    assert manifest["version"] == 2 and manifest["test_only"] is True
    return output


@pytest.mark.skipif(os.environ.get("EREBUS_M5_QUALIFY_FIXTURE") != "1", reason="explicit known-entropy fixture qualification opt-in required")
@pytest.mark.parametrize("kind,public_names", TRANSITIONS)
def test_actual_cached_fixture_prover_reports_metrics_without_witnesses(tmp_path, kind, public_names):
    binary = ROOT / "sdk/shielded/target/debug/erebus-local-prove"
    request, witness = fixture_request(tmp_path, kind, public_names)
    report = module.qualify(binary, module.digest(binary), request, allow_source_build=True)
    assert report["verified"] and report["test_only"] and report["source_build"]
    assert report["cache_hits"] == 3 and report["downloaded_bytes"] == 0
    assert report["proving_ms"] > 0
    assert str(witness) not in json.dumps(report)
    print(json.dumps({"circuit": kind, **report}, sort_keys=True))


@pytest.fixture(scope="module")
def native_fixture_registry(tmp_path_factory):
    build_timeout = wheel_build_timeout()
    builder_spec = importlib.util.spec_from_file_location("metropolis_native_fixture_builder", ROOT / "scripts/build-metropolis-registry.py")
    builder = importlib.util.module_from_spec(builder_spec)
    builder_spec.loader.exec_module(builder)
    stage = tmp_path_factory.mktemp("native-wheel-fixture")
    package = stage / "package"
    package.mkdir()
    shutil.copytree(ROOT / "packaging/erebus-cli/src", package / "src")
    binaries = {name: qualification_binary(crate, name) for crate, names in builder.BINARY_GROUPS.items() for name in names}
    for path in binaries.values():
        builder.validate_binary(path)
    native = {**binaries, "erebus-selfhost": ROOT / "scripts/metropolis-selfhost.sh"}
    version = "0.3.0.dev2026100701"
    (package / "pyproject.toml").write_text(builder.package_metadata(ROOT / "packaging/erebus-cli", version, native))
    (package / "binaries.json").write_text(json.dumps(sorted(native)))
    shutil.copyfile(ROOT / "packaging/metropolis/hatch_build.py", package / "hatch_build.py")
    shutil.copyfile(ROOT / "LICENSE", package / "LICENSE")
    notices, _ = builder.third_party_notices()
    (package / builder.THIRD_PARTY_NOTICES).write_text(notices)
    (package / "bin").mkdir()
    for name, path in native.items():
        shutil.copy2(path, package / "bin" / name)
    subprocess.run([sys.executable, "-m", "hatchling", "build", "-t", "wheel"], cwd=package, check=True, timeout=build_timeout)
    registry = stage / "registry"
    wheels = registry / "wheels"
    shutil.copytree(package / "dist", wheels)
    builder.require_licenses(wheels)
    assert builder.native_wheel_tag(wheels) in {"macosx_11_0_arm64", "linux_x86_64"}
    manifest = {"version": version, "channel": "metropolis-testnet", "published": False,
        "dirty_source": True, "build_profile": "debug",
        "binaries": {name: {"sha256": builder.sha256(path)} for name, path in binaries.items()},
        "launchers": {"erebus-selfhost": {"sha256": builder.sha256(native["erebus-selfhost"])}},
        "wheels": {path.name: {"sha256": builder.sha256(path)} for path in wheels.glob("*.whl")}}
    (registry / "release.json").write_text(json.dumps(manifest))
    return registry


@pytest.mark.skipif(os.environ.get("EREBUS_M5_QUALIFY_WHEEL") != "1", reason="explicit offline installed-wheel fixture opt-in required")
@pytest.mark.parametrize("kind,public_names", TRANSITIONS)
def test_installed_native_wheel_proves_cached_fixtures_without_source_imports(tmp_path, native_fixture_registry, fixture_artifact_bundle, kind, public_names):
    checker_spec = importlib.util.spec_from_file_location("metropolis_actual_native_install", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    request, witness = fixture_request(tmp_path, kind, public_names, fixture_artifact_bundle)
    result = checker.check(native_fixture_registry, prover_request=request, offline_native_prover=True,
        prover_timeout_seconds=int(os.environ.get("EREBUS_M5_QUALIFY_TIMEOUT_SECONDS", "600")))
    assert result["status"] == "verified" and result["scope"] == "native-prover-only"
    assert result["source_imports"] is False and result["services_verified"] is False
    report = result["installed_proving"]
    assert report["verified"] and report["test_only"] and not report["source_build"]
    assert report["rss_scope"] == "prover_child_high_water"
    assert report["cache_hits"] == 3 and report["downloaded_bytes"] == 0
    assert str(witness) not in json.dumps(result)
    (tmp_path / "public-measurement.json").write_text(json.dumps({"circuit": kind, **result}, sort_keys=True))
    print(json.dumps({"circuit": kind, **result}, sort_keys=True))


@pytest.mark.skipif(os.environ.get("EREBUS_M5_QUALIFY_WHEEL") != "1", reason="explicit offline installed-wheel fixture opt-in required")
@pytest.mark.parametrize("mutation", ["publisher", "signature", "manifest"])
def test_installed_prover_rejects_changed_manifest_authorization(tmp_path, native_fixture_registry, fixture_artifact_bundle, mutation):
    checker_spec = importlib.util.spec_from_file_location("metropolis_installed_auth_refusal", ROOT / "scripts/check-metropolis-install.py")
    checker = importlib.util.module_from_spec(checker_spec)
    checker_spec.loader.exec_module(checker)
    kind, public_names = TRANSITIONS[0]
    request, _ = fixture_request(tmp_path, kind, public_names, fixture_artifact_bundle)
    payload = json.loads(request.read_text())
    if mutation == "publisher":
        payload["manifest_authorization"]["publisher"] = "0x" + "12" * 20
    elif mutation == "signature":
        payload["manifest_authorization"]["signature"]["signature_hex"] = "00" * 65
    else:
        manifest = Path(payload["manifest_file"])
        manifest.write_bytes(manifest.read_bytes() + b"\n")
        payload["manifest_sha256"] = module.digest(manifest)
    request.write_text(json.dumps(payload))
    with pytest.raises(ValueError, match="installed proving qualification failed"):
        checker.check(native_fixture_registry, prover_request=request, offline_native_prover=True)
