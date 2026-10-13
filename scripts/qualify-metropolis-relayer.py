#!/usr/bin/env python3
"""Installed-relayer qualification report: the packaged erebus-shielded-relayer on a loopback Anvil chain 10143.

    qualify-metropolis-relayer.py --registry DIR --version V --work NEW [--checkout DIR]

Installs `erebus-mcp-server==V` from the local registry into a new environment outside the checkout and checks the
installed relayer against the registry's release.json. It then runs the checkout's `relayer_binary` fixture tests
(cargo, release profile) with `EREBUS_TEST_SHIELDED_RELAYER` and `EREBUS_TEST_PAYMENT` set to the installed paths,
so the tests drive the packaged binaries, never an in-process service or a cargo-built copy. The report on stdout records:
- release.json SHA-256, source commit and the installed relayer and payment hashes;
- the fixture files' hashes and each test's result;
- every gate check with its exact scope and the test that covers it.

What these tests do not cover is reported as such. In particular the buyer test drives the `RelayEndpoint`
transport, not the installed `erebus-payment` CLI or the MCP `sponsor_shielded_deal` tool, so
`installed_payment_delegation` stays false until a fixture drives those. Publication refuses that.
Loopback Anvil and test keys only: never live Monad evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LABELS = ["test-only", "single-operator", "unaudited", "local-anvil", "installed-relayer"]
FIXTURES = ("sdk/shielded/tests/relayer_binary.rs", "circuits/m5/scripts/native-fixture.mjs",
            "circuits/m5/scripts/relay-admission-fixture.mjs")
SUBMIT = "installed_relayer_binary_submits_observes_restarts_and_recovers_finalized_receipt"
RESUME = "installed_relayer_binary_recovers_a_dropped_broadcast_with_explicit_resume"
DUPLICATE = "installed_relayer_binary_serves_duplicate_requests_with_one_broadcast"
TRANSPORT = "buyer_transport_delegates_to_the_installed_binary_and_never_signs_directly"
OVERSIZED = "installed_relayer_binary_answers_an_oversized_body_without_a_reset"
GATES = "fixture_scripts_refuse_non_loopback_endpoints_and_foreign_chains"
FUNDED = (SUBMIT, RESUME, DUPLICATE, TRANSPORT, OVERSIZED)
# check -> (covering test, exact scope). The scope is what the test asserts, no more.
CHECKS = {
    "submit": (SUBMIT, "installed relayer accepts a proof-only request and sends one sponsor transaction"),
    "status": (SUBMIT, "observe reports pending, then finalized with the same transaction hash"),
    "restart_recovery": (SUBMIT, "relayer restart keeps its journal; finalized receipt recovered after restart"),
    "sponsor_sender": (SUBMIT, "receipt from the sponsor to the pool with exact calldata, value 0, status 1, one payment"),
    "dropped_broadcast_resumed": (RESUME, "dropped first broadcast; owner --resume resends identical bytes exactly once"),
    "duplicate_one_broadcast": (DUPLICATE, "concurrent and sequential duplicates: one raw broadcast, one hash, finalized once"),
    "no_direct_fallback": (TRANSPORT, "buyer RelayEndpoint transport only: buyer nonce stays 0 and a stopped relayer "
                                      "returns unknown with no buyer send; NOT the installed erebus-payment CLI or MCP"),
    "oversized_body_413": (OVERSIZED, "a 2 MiB body receives HTTP 413"),
    "loopback_and_chain_refusal": (GATES, "fixture scripts refuse non-loopback endpoints and foreign chains"),
}
# Tests that drive the installed erebus-payment (or MCP) through the installed relayer. Empty until OpenCode freezes
# one; the transport test never belongs here. Each must pass for `installed_payment_delegation`.
DELEGATION_TESTS: tuple[str, ...] = ()
DELEGATION_SCOPE = ("installed erebus-payment delegating to the installed relayer: delegation persisted before "
                    "transport, timeout or lost response, buyer restart, relay unavailable, recovery, no direct "
                    "fallback, exactly one payment")
MISSING = {
    "installed_payment_delegation": "installed erebus-payment (and MCP sponsor_shielded_deal) delegating to the installed "
                                    "relayer: delegation persisted before transport, timeout or lost response, buyer "
                                    "restart, relay unavailable, recovery, no direct fallback, exactly one payment",
}
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.M)
EVIDENCE = re.compile(r"^EREBUS_EVIDENCE (\{.*\})$", re.M)
PINNED = {"erebus-shielded-relayer": "EREBUS_TEST_SHIELDED_RELAYER", "erebus-payment": "EREBUS_TEST_PAYMENT"}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cargo_tests(checkout: Path, binaries: dict[str, Path], arguments: list[str], log: Path) -> dict[str, str]:
    cargo = shutil.which("cargo")
    if cargo is None or shutil.which("anvil") is None:
        raise ValueError("the relayer fixture requires cargo and Anvil")
    environment = {key: value for key, value in os.environ.items() if not key.startswith("EREBUS_")}
    for name, variable in PINNED.items():
        environment[variable] = str(binaries[name])
    result = subprocess.run([cargo, "test", "--release", "--locked", "--manifest-path",
                             str(checkout / "sdk/shielded/Cargo.toml"), "--test", "relayer_binary", "--", *arguments],
                            cwd=checkout, env=environment, capture_output=True, text=True, timeout=3600, check=False)
    with log.open("a") as handle:
        handle.write(result.stdout + result.stderr)
    return {name: outcome for name, outcome in RESULT.findall(result.stdout)}


def evidence_lines(log: Path) -> list[dict]:
    """Machine-readable `EREBUS_EVIDENCE {...}` lines a fixture printed, kept verbatim; unparsable ones are dropped."""
    found = []
    for line in EVIDENCE.findall(log.read_text() if log.exists() else ""):
        try:
            found.append(json.loads(line))
        except ValueError:
            continue
    return found


def qualify(registry: Path, version: str, work: Path, checkout: Path, run=cargo_tests, install=None) -> dict:
    release_path = registry / "release.json"
    release = json.loads(release_path.read_text())
    expected = (release.get("binaries") or {}).get("erebus-shielded-relayer", {}).get("sha256")
    if expected is None:
        raise ValueError("these packages do not include erebus-shielded-relayer")
    work.mkdir(mode=0o700)
    if install is None:
        spec = importlib.util.spec_from_file_location("check_metropolis_init", ROOT / "scripts/check-metropolis-init.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        install = module.install
    bins = install(work, version, registry, None)
    pinned = {}
    for name in PINNED:
        path = bins / name
        record = (release.get("binaries") or {}).get(name, {}).get("sha256")
        if path.is_symlink() or record is None or sha256(path) != record:
            raise ValueError(f"the installed {name} is not the packaged binary")
        if path.resolve().is_relative_to(checkout.resolve()):
            raise ValueError(f"the {name} under test must be installed outside the checkout")
        pinned[name] = path
    log = work / "relayer-tests.log"
    results = run(checkout, pinned, ["--ignored", "--test-threads=1"], log)
    results.update(run(checkout, pinned, ["--exact", GATES], log))
    checks = {}
    for check, (test, scope) in CHECKS.items():
        checks[check] = {"passed": results.get(test) == "ok", "scope": scope, "test": test}
    if TRANSPORT in DELEGATION_TESTS:
        raise ValueError("the transport-only test cannot stand in for installed payment delegation")
    if DELEGATION_TESTS:
        checks["installed_payment_delegation"] = {
            "passed": all(results.get(test) == "ok" for test in DELEGATION_TESTS), "scope": DELEGATION_SCOPE,
            "test": ",".join(DELEGATION_TESTS)}
    else:
        for check, scope in MISSING.items():
            checks[check] = {"passed": False, "scope": f"not covered by this fixture: {scope}", "test": None}
    ran = [*FUNDED, *DELEGATION_TESTS, GATES]
    passed = all(results.get(name) == "ok" for name in ran)
    native = (os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"
              and release.get("dirty_source") is False)
    return {
        "status": "verified" if passed else "failed",
        "gate_complete": passed and all(entry["passed"] for entry in checks.values()),
        "coverage_gaps": sorted(name for name, entry in checks.items() if not entry["passed"]),
        "service": "installed-binary", "in_process": False,
        "source_imports": False, "fixture_harness_from_checkout": True,
        "release_json_sha256": sha256(release_path), "version": release.get("version"),
        "source_commit": release.get("source_commit"),
        "candidate_source_tree_sha256": (release.get("candidate") or {}).get("source_tree_sha256"),
        "installed_binaries_sha256": {name: sha256(path) for name, path in pinned.items()},
        "fixture_evidence": evidence_lines(log),
        "fixture_files_sha256": {name: sha256(checkout / name) for name in FIXTURES},
        "tests": {name: results.get(name, "not run") for name in ran},
        "checks": checks,
        "claims": {"native_platform_qualification": native, "live_payment": False,
                   "note": "loopback Anvil chain 10143 with fixture keys; not live Monad or relayed settlement evidence"},
        "labels": LABELS,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--checkout", type=Path, default=ROOT, help="tree holding the relayer_binary fixture")
    arguments = parser.parse_args()
    try:
        report = qualify(arguments.registry.resolve(), arguments.version, arguments.work.resolve(),
                         arguments.checkout.resolve())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report = {"status": "failed", "error": f"{type(error).__name__}: {error}", "labels": LABELS}
    print(json.dumps(report, indent=2))
    return 0 if report["status"] == "verified" else 1


if __name__ == "__main__":
    sys.exit(main())
