#!/usr/bin/env python3
"""Install only from a local Metropolis index; verify binaries and MCP outside the repo."""

from __future__ import annotations

import argparse
import functools
import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class RegistryHandler(SimpleHTTPRequestHandler):
    def log_message(self, *args) -> None:
        pass


def recorded_proving(manifest: dict) -> list[dict]:
    """Every proving manifest the release recorded, one per pool domain (older releases record one)."""
    if manifest.get("proving_domains"):
        return list(manifest["proving_domains"])
    return [manifest["proving_artifacts"]] if manifest.get("proving_artifacts") else []


def recorded_proving_sha256(manifest: dict) -> set[str]:
    return {record["manifest_sha256"] for record in recorded_proving(manifest)}


def evidence(manifest: dict) -> dict:
    """What this run can and cannot claim. A local check is never outside acceptance."""
    candidate = manifest.get("candidate") or {}
    return {
        "source_commit": manifest.get("source_commit"),
        "merge_head": candidate.get("merge_head"),
        "dirty_source": manifest.get("dirty_source"),
        "source_tree_sha256": candidate.get("source_tree_sha256"),
        "build_profile": manifest.get("build_profile"),
        "wheel_tag": manifest.get("platform_qualification", {}).get("wheel_tag"),
        "release_eligible": candidate.get("release_eligible", False),
        "evidence_scope": "team-rehearsal",
        "outside_acceptance": False,
        "proving_manifest_sha256": (manifest.get("proving_artifacts") or {}).get("manifest_sha256"),
        "proving_artifacts_test_only": (manifest.get("proving_artifacts") or {}).get("test_only"),
        "binaries_sha256": {name: record["sha256"] for name, record in sorted((manifest.get("binaries") or {}).items())},
        "launchers_sha256": {name: record["sha256"] for name, record in sorted((manifest.get("launchers") or {}).items())},
        "deployment_schemas": {name: schema.get("fields") for name, schema in (manifest.get("deployment_schemas") or {}).items()},
        "proving_domains": [{"manifest_sha256": record.get("manifest_sha256"), "chain_id": record.get("chain_id"),
                             "settlement_contract": record.get("settlement_contract"), "test_only": record.get("test_only")}
                            for record in recorded_proving(manifest)],
    }


def require_recorded_proving_manifest(manifest: dict, request: Path) -> None:
    """A prover request must name the proving manifest the release recorded, when it recorded one."""
    recorded = recorded_proving_sha256(manifest)
    if not recorded:
        return
    try:
        pinned = json.loads(request.read_text()).get("manifest_sha256")
    except (OSError, ValueError, AttributeError) as error:
        raise ValueError("installed proving qualification failed") from error
    if pinned not in recorded:
        raise ValueError("prover request names a different proving manifest than the release")


def qualify_installed_prover(bins: Path, manifest: dict, request: Path, environment: dict,
                             timeout_seconds: int = 600, check_recorded: bool = True) -> dict:
    if type(timeout_seconds) is not int or not 1 <= timeout_seconds <= 3600:
        raise ValueError("qualification timeout must be between 1 and 3600 seconds")
    if check_recorded:
        require_recorded_proving_manifest(manifest, request)
    binary = bins / "erebus-local-prove"
    expected = manifest["binaries"]["erebus-local-prove"]["sha256"]
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("binary identity mismatch")
    with binary.open("rb") as stream:
        if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
            raise ValueError("binary identity mismatch")
    # A fresh runner has no earlier children whose peak RSS could mask the prover's.
    try:
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("qualify-metropolis-prover.py")),
            "--binary", str(binary.resolve()), "--binary-sha256", expected, "--request", str(request.resolve()),
            "--timeout", str(timeout_seconds)],
            env=environment, cwd=bins.parent, capture_output=True, text=True, timeout=timeout_seconds + 60, check=False)
        if len(result.stdout) > 65536:
            raise ValueError("installed proving qualification failed")
        report = json.loads(result.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError) as error:
        raise ValueError("installed proving qualification failed") from error
    if (result.returncode != 0 or not isinstance(report, dict) or report.get("verified") is not True
            or report.get("source_build") is not False or report.get("rss_scope") != "prover_child_high_water"):
        raise ValueError("installed proving qualification failed")
    return report


TRANSITIONS = {
    "deposit": ["chainId", "contractAddress", "verifierVersion", "asset", "amount", "noteCommitment"],
    "transfer": ["chainId", "contractAddress", "verifierVersion", "asset", "dealCommitment", "dealNullifier", "root",
                 "inputNullifier", "paymentCommitment", "changeCommitment", "expiry"],
    "withdraw": ["chainId", "contractAddress", "verifierVersion", "asset", "root", "noteNullifier", "recipient", "amount"],
}


def artifact_path(kind: str, field: str) -> str:
    return f"{kind}_js/{kind}.wasm" if field == "wasm" else f"{kind}.{field}"


def rehearse_cold_proving(bins: Path, manifest: dict, bundle: Path, witnesses: Path, environment: dict,
                          root: Path, timeout_seconds: int) -> dict:
    """Download, authenticate, and prove from installed binaries with empty caches.

    The bundle's files are served over loopback under a copy of its manifest whose URLs point
    at the local server, signed by an ephemeral publisher key through installed
    `erebus-artifacts`. Every file identity in that copy must equal the manifest the release
    recorded. Loopback HTTP is a development exception, so only test-only bundles are accepted.
    """
    source_bytes = (bundle / "manifest.json").read_bytes()
    source_sha256 = hashlib.sha256(source_bytes).hexdigest()
    if source_sha256 not in recorded_proving_sha256(manifest):
        raise ValueError("cold proving needs the bundle whose manifest the release recorded")
    source = json.loads(source_bytes)
    if source.get("test_only") is not True:
        raise ValueError("loopback artifact distribution is limited to test-only bundles")
    served = root / "served artifacts"
    served.mkdir(mode=0o700)
    tampered = root / "tampered artifacts"
    tampered.mkdir(mode=0o700)
    for circuit in source["circuits"]:
        for field in ("wasm", "r1cs", "zkey"):
            relative = artifact_path(circuit["kind"], field)
            for directory in (served, tampered):
                (directory / relative).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(bundle / relative, directory / relative)
    zkey = tampered / artifact_path("transfer", "zkey")
    data = bytearray(zkey.read_bytes())
    data[len(data) // 2] ^= 0x01
    zkey.write_bytes(bytes(data))
    servers = {}
    try:
        for name, directory in (("served", served), ("tampered", tampered)):
            server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(RegistryHandler, directory=str(directory)))
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            servers[name] = (server, thread)
        manifests = {}
        for name, (server, _) in servers.items():
            copy = json.loads(source_bytes)
            for circuit in copy["circuits"]:
                for field in ("wasm", "r1cs", "zkey"):
                    circuit[field]["url"] = f"http://127.0.0.1:{server.server_port}/{artifact_path(circuit['kind'], field)}"
            path = root / f"{name}-manifest.json"
            path.write_text(json.dumps(copy, indent=2) + "\n")
            for original, rewritten in zip(source["circuits"], copy["circuits"]):
                for field in ("wasm", "r1cs", "zkey"):
                    if {k: v for k, v in original[field].items() if k != "url"} != {k: v for k, v in rewritten[field].items() if k != "url"}:
                        raise ValueError("served manifest changed an artifact identity")
            manifests[name] = path
        publisher_key = root / "ephemeral-publisher.key"
        publisher_key.write_bytes(os.urandom(32))
        publisher_key.chmod(0o600)
        authorizations = {}
        try:
            for name, path in manifests.items():
                signed = subprocess.run([str(bins / "erebus-artifacts"), "--sign-manifest"],
                                        input=json.dumps({"manifest_file": str(path), "publisher_key_file": str(publisher_key)}),
                                        env=environment, cwd=root, text=True, capture_output=True, timeout=30, check=True)
                reply = json.loads(signed.stdout)
                if reply.get("status") != "ok":
                    raise ValueError("installed manifest signing failed")
                authorizations[name] = {"publisher": reply["publisher"], "signature": reply["signature"]}
        finally:
            publisher_key.unlink(missing_ok=True)

        def request(name: str, kind: str, cache: Path) -> Path:
            witness = root / f"{kind}-witness.json"
            if not witness.exists():
                shutil.copyfile(witnesses / f"{kind}-input.json", witness)
                witness.chmod(0o600)
            values = json.loads(witness.read_text())
            path = root / f"{name}-{kind}-{cache.name}-request.json"
            path.write_text(json.dumps({
                "manifest_file": str(manifests[name]), "manifest_sha256": hashlib.sha256(manifests[name].read_bytes()).hexdigest(),
                "manifest_authorization": authorizations[name], "circuit": kind, "cache_root": str(cache),
                "witness_file": str(witness), "expected_public": [values[field] for field in TRANSITIONS[kind]],
                "allow_test_artifacts": True, "allow_loopback_http": True,
            }))
            path.chmod(0o600)
            return path

        def public(report: dict) -> dict:
            keys = ("elapsed_ms", "proving_ms", "installation_ms", "downloaded_bytes", "cache_hits",
                    "child_cpu_ms", "child_peak_rss_bytes", "test_only", "source_build", "verified")
            return {key: report.get(key) for key in keys}

        results = {}
        for kind in TRANSITIONS:
            cache = root / f"cold-cache-{kind}"
            cache.mkdir(mode=0o700)
            report = qualify_installed_prover(bins, manifest, request("served", kind, cache), environment,
                                              timeout_seconds, check_recorded=False)
            if report.get("downloaded_bytes", 0) <= 0 or report.get("cache_hits") != 0 or report.get("test_only") is not True:
                raise ValueError("cold proving did not download and authenticate every artifact")
            results[kind] = {"cold": public(report)}
        warm = root / "cold-cache-transfer"
        report = qualify_installed_prover(bins, manifest, request("served", "transfer", warm), environment,
                                          timeout_seconds, check_recorded=False)
        if report.get("downloaded_bytes") != 0 or report.get("cache_hits") != 3:
            raise ValueError("retained artifacts were not reused after verification")
        results["transfer"]["warm"] = public(report)
        rejected = root / "tampered-cache"
        rejected.mkdir(mode=0o700)
        try:
            qualify_installed_prover(bins, manifest, request("tampered", "transfer", rejected), environment,
                                     timeout_seconds, check_recorded=False)
        except ValueError:
            tamper_rejected = True
        else:
            tamper_rejected = False
        tampered_sha256 = hashlib.sha256(bytes(data)).hexdigest()
        retained = [path for path in rejected.rglob("*") if path.is_file()
                    and hashlib.sha256(path.read_bytes()).hexdigest() == tampered_sha256]
        if not tamper_rejected or retained:
            raise ValueError("a tampered served artifact was accepted or retained")
    finally:
        for server, thread in servers.values():
            server.shutdown()
            server.server_close()
            thread.join()
    return {"source_manifest_sha256": source_sha256, "served_over": "loopback-http", "publisher": "ephemeral",
            "test_only": True, "transitions": results, "tampered_artifact_rejected": True}


def load_staging_module():
    """Reuse the hosting rules from the artifact stager rather than restating them here."""
    import importlib.util
    path = Path(__file__).resolve().parents[1] / "packaging/metropolis/stage_artifacts.py"
    spec = importlib.util.spec_from_file_location("metropolis_stage_artifacts", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def flip_middle_byte(path: Path) -> str:
    data = bytearray(path.read_bytes())
    data[len(data) // 2] ^= 0x01
    path.write_bytes(bytes(data))
    return hashlib.sha256(bytes(data)).hexdigest()


def witness_domain(witnesses: Path) -> tuple[int, str, int]:
    """The (chain, contract, verifier version) every fixture witness in the directory is bound to."""
    domains = set()
    for kind in TRANSITIONS:
        values = json.loads((witnesses / f"{kind}-input.json").read_text())
        domains.add((int(values["chainId"]), f"0x{int(values['contractAddress']):040x}", int(values["verifierVersion"])))
    if len(domains) != 1:
        raise ValueError("fixture witnesses name more than one domain")
    return domains.pop()


def manifest_domain(manifest: dict) -> tuple[int, str, int]:
    return int(manifest["chain_id"]), str(manifest["settlement_contract"]).lower(), int(manifest["verifier_version"])


def install_artifacts(bins: Path, release: dict, request: Path, environment: dict, timeout_seconds: int) -> dict:
    """Install one circuit's artifacts with the hash-checked installed `erebus-artifacts`; no proving."""
    binary = bins / "erebus-artifacts"
    if binary.is_symlink() or not binary.is_file() or \
            hashlib.sha256(binary.read_bytes()).hexdigest() != release["binaries"]["erebus-artifacts"]["sha256"]:
        raise ValueError("binary identity mismatch")
    body = {key: value for key, value in json.loads(request.read_text()).items()
            if key in {"manifest_file", "manifest_sha256", "manifest_authorization", "circuit", "cache_root",
                       "allow_test_artifacts", "allow_loopback_http"}}
    result = subprocess.run([str(binary)], input=json.dumps(body), env=environment, cwd=bins.parent, text=True,
                            capture_output=True, timeout=timeout_seconds, check=False)
    try:
        reply = json.loads(result.stdout)
    except ValueError:
        reply = {}
    if result.returncode != 0 or reply.get("status") != "ok":
        raise ValueError("installed artifact installation refused")
    manifest = json.loads(Path(body["manifest_file"]).read_bytes())
    circuit = next(item for item in manifest["circuits"] if item["kind"] == body["circuit"])
    if any(reply.get(f"{field}_sha256") != circuit[field]["sha256"] for field in ("wasm", "r1cs", "zkey")):
        raise ValueError("installed artifacts differ from the manifest")
    # The installer accepted the manifest under this opt-in; it does not echo the boundary itself.
    return {"downloaded_bytes": reply.get("downloaded_bytes"), "cache_hits": reply.get("cache_hits"),
            "test_only": manifest.get("test_only"), "verified": True}


def prove_against_manifest(bins: Path, release: dict, manifest_path: Path, authorization: dict, witnesses: Path,
                           environment: dict, root: Path, timeout_seconds: int, loopback: bool) -> dict:
    """Empty-cache installs, warm reuse, cache tamper, and authorization refusals against one manifest.

    The manifest's own URLs are used unchanged. Callers decide whether they name real HTTPS hosting
    or a loopback fixture; only a fixture may set `loopback`.

    The installed prover refuses public inputs from another domain, so proofs run only when the fixture
    witnesses are bound to this manifest's domain. Otherwise every step uses installed `erebus-artifacts`
    (download, authentication, cache), the report says `proved: false`, and a proof attempt with the
    foreign-domain witness must be refused before anything is installed.
    """
    manifest = json.loads(manifest_path.read_bytes())
    test_only = manifest.get("test_only")
    if type(test_only) is not bool:
        raise ValueError("hosted manifest must declare its fixture boundary")
    files = {(circuit["kind"], field): circuit[field] for circuit in manifest["circuits"] for field in ("wasm", "r1cs", "zkey")}
    proving = witness_domain(witnesses) == manifest_domain(manifest)

    def request(kind: str, cache: Path, manifest_file: Path = manifest_path, auth: dict = authorization,
                foreign_domain: bool = False) -> Path:
        witness = root / f"{kind}-witness.json"
        if not witness.exists():
            shutil.copyfile(witnesses / f"{kind}-input.json", witness)
            witness.chmod(0o600)
        values = json.loads(witness.read_text())
        if foreign_domain:
            # Public inputs naming a neighbouring pool address: the prover must refuse before installing.
            values["contractAddress"] = str(int(values["contractAddress"]) ^ 1)
        path = root / f"{kind}-{cache.name}-{manifest_file.stem}-request.json"
        path.write_text(json.dumps({
            "manifest_file": str(manifest_file), "manifest_sha256": hashlib.sha256(manifest_file.read_bytes()).hexdigest(),
            "manifest_authorization": auth, "circuit": kind, "cache_root": str(cache), "witness_file": str(witness),
            "expected_public": [values[field] for field in TRANSITIONS[kind]],
            "allow_test_artifacts": test_only, "allow_loopback_http": loopback,
        }))
        path.chmod(0o600)
        return path

    def prove(kind: str, cache: Path, **overrides) -> dict:
        if not proving:
            return install_artifacts(bins, release, request(kind, cache, **overrides), environment, timeout_seconds)
        return qualify_installed_prover(bins, release, request(kind, cache, **overrides), environment,
                                        timeout_seconds, check_recorded=False)

    def refused(kind: str, cache: Path, **overrides) -> bool:
        try:
            prove(kind, cache, **overrides)
        except ValueError:
            return True
        return False

    keys = ("elapsed_ms", "proving_ms", "installation_ms", "downloaded_bytes", "cache_hits", "test_only", "verified")
    results = {}
    for kind in TRANSITIONS:
        cache = root / f"hosted-cache-{kind}"
        cache.mkdir(mode=0o700)
        report = prove(kind, cache)
        expected = sum(files[(kind, field)]["bytes"] for field in ("wasm", "r1cs", "zkey"))
        if report.get("downloaded_bytes") != expected or report.get("cache_hits") != 0 or report.get("test_only") is not test_only:
            raise ValueError("hosted install did not download and authenticate every artifact")
        results[kind] = {"cold": {key: report.get(key) for key in keys}}
    cache = root / "hosted-cache-transfer"
    report = prove("transfer", cache)
    if report.get("downloaded_bytes") != 0 or report.get("cache_hits") != 3:
        raise ValueError("hosted artifacts were not reused after verification")
    results["transfer"]["warm"] = {key: report.get(key) for key in keys}
    zkey = files[("transfer", "zkey")]
    cached = cache / f"{zkey['sha256']}.artifact"
    tampered_sha256 = flip_middle_byte(cached)
    try:
        report = prove("transfer", cache)
        outcome = "redownloaded"
        if report.get("downloaded_bytes") != zkey["bytes"] or report.get("cache_hits") != 2:
            raise ValueError("a tampered cached artifact was used")
    except ValueError as error:
        if "tampered cached" in str(error):
            raise
        outcome = "refused"
    if cached.exists() and hashlib.sha256(cached.read_bytes()).hexdigest() == tampered_sha256:
        raise ValueError("a tampered cached artifact was retained")
    wrong = dict(authorization, publisher="0x" + os.urandom(20).hex())
    changed = root / "changed-manifest.json"
    changed.write_bytes(manifest_path.read_bytes() + b"\n")
    refusals = {
        "wrong_publisher": refused("deposit", root / "refusal-cache-publisher", auth=wrong),
        "changed_manifest_bytes": refused("deposit", root / "refusal-cache-bytes", manifest_file=changed),
    }
    if not all(refusals.values()):
        raise ValueError(f"hosted manifest authorization was not enforced: {refusals}")
    # Wrong domain: a foreign-domain witness when the fixture witnesses belong elsewhere, otherwise this
    # domain's witness with public inputs that name another pool. Either must be refused before install.
    cache = root / "refusal-cache-domain"
    cache.mkdir(mode=0o700)
    try:
        qualify_installed_prover(bins, release, request("deposit", cache, foreign_domain=proving), environment,
                                 timeout_seconds, check_recorded=False)
        foreign = False
    except ValueError:
        foreign = True
    refusals["foreign_domain_public_inputs" if proving else "foreign_domain_witness"] = foreign
    if not foreign:
        raise ValueError("the installed prover accepted a witness from another domain")
    for path in (root / "refusal-cache-publisher", root / "refusal-cache-bytes", root / "refusal-cache-domain"):
        if path.exists() and any(path.glob("*.artifact")):
            raise ValueError("a refused manifest left installed artifacts")
    chain, contract, version = witness_domain(witnesses)
    return {"manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(), "test_only": test_only,
            "proved": proving, "witness_domain": {"chain_id": chain, "settlement_contract": contract, "verifier_version": version},
            "transitions": results, "cache_tamper": outcome, "authorization_refusals": refusals}


class RedirectingReleaseHandler(SimpleHTTPRequestHandler):
    """Answer `/releases/download/<tag>/<name>` with a 302 to `/objects/<name>`, like GitHub release assets."""
    redirects = 0

    def do_GET(self) -> None:
        if self.path.startswith("/releases/download/"):
            type(self).redirects += 1
            self.send_response(302)
            self.send_header("Location", "/objects/" + self.path.rsplit("/", 1)[1])
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        super().do_GET()

    def log_message(self, *args) -> None:
        pass


def serve_release(directory: Path) -> tuple[ThreadingHTTPServer, threading.Thread, type]:
    handler = type("FixtureRelease", (RedirectingReleaseHandler,), {"redirects": 0})
    server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(handler, directory=str(directory)))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread, handler


def sign_with_ephemeral_publisher(bins: Path, manifest_path: Path, environment: dict, root: Path) -> dict:
    key = root / "ephemeral-publisher.key"
    key.write_bytes(os.urandom(32))
    key.chmod(0o600)
    try:
        signed = subprocess.run([str(bins / "erebus-artifacts"), "--sign-manifest"],
                                input=json.dumps({"manifest_file": str(manifest_path), "publisher_key_file": str(key)}),
                                env=environment, cwd=root, text=True, capture_output=True, timeout=30, check=True)
    finally:
        key.unlink(missing_ok=True)
    reply = json.loads(signed.stdout)
    if reply.get("status") != "ok":
        raise ValueError("installed manifest signing failed")
    return {"publisher": reply["publisher"], "signature": reply["signature"]}


def rehearse_hosted_fixture(bins: Path, release: dict, bundle: Path, witnesses: Path, environment: dict,
                            root: Path, timeout_seconds: int, allow_unrecorded: bool = False) -> dict:
    """The hosted-download path against a local redirecting release server (loopback HTTP, ephemeral publisher).

    Files are staged with the same flat names and URL layout `stage_artifacts.py` produces for GitHub
    release assets. The installed binary's bundled TLS roots cannot trust a local certificate, so the
    HTTPS hop itself is exercised only against real hosting.

    `allow_unrecorded` exercises a pool domain the packages did not record (domain rehearsal before a
    rebuild). The report then says `release_recorded: false`, which no qualification gate accepts.
    """
    staging = load_staging_module()
    source_bytes = (bundle / "manifest.json").read_bytes()
    source_sha256 = hashlib.sha256(source_bytes).hexdigest()
    release_recorded = source_sha256 in recorded_proving_sha256(release)
    if not release_recorded and not allow_unrecorded:
        raise ValueError("hosted fixture needs the bundle whose manifest the release recorded")
    if json.loads(source_bytes).get("test_only") is not True:
        raise ValueError("loopback artifact distribution is limited to test-only bundles")
    servers = []
    try:
        reports = {}
        for name in ("hosted", "tampered"):
            directory = root / f"{name}-release"
            objects = directory / "objects"
            objects.mkdir(parents=True, mode=0o700)
            server, thread, handler = serve_release(directory)
            servers.append((server, thread))
            base = f"http://127.0.0.1:{server.server_port}/releases/download/fixture-{name}/"
            # Same flat asset names as `stage_artifacts.py stage`; that command refuses loopback hosts.
            manifest = json.loads(source_bytes)
            for circuit in manifest["circuits"]:
                for field in ("wasm", "r1cs", "zkey"):
                    flat = f"{circuit['kind']}.{field}"
                    shutil.copyfile(bundle / staging.bundle_path(circuit["kind"], field), objects / flat)
                    circuit[field]["url"] = base + flat
            manifest_path = root / f"{name}-manifest.json"
            manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
            source = json.loads(source_bytes)
            for before, after in zip(source["circuits"], manifest["circuits"]):
                for field in ("wasm", "r1cs", "zkey"):
                    if {k: v for k, v in before[field].items() if k != "url"} != {k: v for k, v in after[field].items() if k != "url"}:
                        raise ValueError("fixture manifest changed an artifact identity")
            authorization = sign_with_ephemeral_publisher(bins, manifest_path, environment, root)
            if name == "hosted":
                proving_root = root / "hosted-proving"
                proving_root.mkdir(mode=0o700)
                reports = prove_against_manifest(bins, release, manifest_path, authorization, witnesses,
                                                 environment, proving_root, timeout_seconds, loopback=True)
                if handler.redirects < 9:
                    raise ValueError("fixture downloads did not follow release redirects")
                reports["redirects_followed"] = handler.redirects
            else:
                tampered_sha256 = flip_middle_byte(objects / "transfer.zkey")
                witness_root = root / "served-tamper"
                witness_root.mkdir(mode=0o700)
                try:
                    prove_against_manifest(bins, release, manifest_path, authorization, witnesses, environment,
                                           witness_root, timeout_seconds, loopback=True)
                    served_tamper_refused = False
                except ValueError:
                    served_tamper_refused = True
                retained = [path for path in witness_root.rglob("*.artifact")
                            if hashlib.sha256(path.read_bytes()).hexdigest() == tampered_sha256]
                if not served_tamper_refused or retained:
                    raise ValueError("a tampered hosted artifact was accepted or retained")
                reports["served_tamper_refused"] = True
    finally:
        for server, thread in servers:
            server.shutdown()
            server.server_close()
            thread.join()
    source = json.loads(source_bytes)
    return {"source_manifest_sha256": source_sha256, "release_recorded": release_recorded,
            "chain_id": source.get("chain_id"), "settlement_contract": source.get("settlement_contract"),
            "served_over": "loopback-http-redirects", "publisher": "ephemeral", "https_exercised": False, **reports}


def rehearse_hosted(bins: Path, release: dict, manifest_path: Path, pin: str, publisher: str, signature_file: Path,
                    witnesses: Path, environment: dict, root: Path, timeout_seconds: int) -> dict:
    """Real hosted artifacts: the release must record exactly this signed hosting manifest."""
    staging = load_staging_module()
    data = manifest_path.read_bytes()
    if hashlib.sha256(data).hexdigest() != pin:
        raise ValueError("hosted manifest does not match its pin")
    if pin not in recorded_proving_sha256(release):
        raise ValueError("the release did not record this hosted manifest")
    for circuit in json.loads(data)["circuits"]:
        for field in ("wasm", "r1cs", "zkey"):
            staging.require_hostable_base_url(circuit[field]["url"].rsplit("/", 1)[0] + "/")
    authorization = {"publisher": publisher, "signature": json.loads(signature_file.read_text())}
    report = prove_against_manifest(bins, release, manifest_path, authorization, witnesses, environment, root,
                                    timeout_seconds, loopback=False)
    manifest = json.loads(data)
    return {"served_over": "https", "publisher": publisher, "https_exercised": True, "release_recorded": True,
            "chain_id": manifest.get("chain_id"), "settlement_contract": manifest.get("settlement_contract"), **report}


def rehearse_onboarding(bins: Path, environment: dict, root: Path) -> dict:
    """Suite-2 operator setup, private identity bindings, and an auditor key from installed binaries."""
    import stat
    import time

    def call(binary: str, request: dict) -> dict:
        result = subprocess.run([str(bins / binary)], input=json.dumps(request), env=environment, cwd=root,
                                text=True, capture_output=True, timeout=30, check=False)
        reply = json.loads(result.stdout)
        if result.returncode != 0 or reply.get("status") != "ok":
            raise ValueError(f"installed onboarding failed at {binary} {request.get('method')}")
        return reply

    expires = int(time.time()) + 3600
    roles = {}
    for role, port in (("buyer", 9001), ("seller", 9002)):
        directory = root / f"new-{role}"
        reply = call("erebus-negotiate", {"method": "prepare_operator", "suite_id": 2, "directory": str(directory),
                     "role": role, "endpoint": f"127.0.0.1:{port}", "namespace": "eip155:10143",
                     "assets": ["eip155:10143/erc20:0x" + "11" * 20], "descriptor_lifetime_seconds": 86400})
        expected = {"agreement_key_file", "wallet_agreement_key_file", "wallet_key_file", "discovery_key_file", "transport_key_file"}
        if role == "seller":
            expected.add("seller_spend_secret_file")
        if not expected <= reply.keys() or reply.get("suite_id") != 2:
            raise ValueError("installed onboarding returned an incomplete suite-2 operator")
        call("erebus-negotiate", {"method": "prepare_binding", "output": str(directory / "identity.binding"),
             "descriptor_file": reply["descriptor_file"], "discovery_key_file": reply["discovery_key_file"],
             "agreement_key_file": reply["agreement_key_file"],
             "settlement_contract": "0x" + "12" * 20, "verifier_version": 2, "expires_at": expires})
        private = [Path(reply[name]) for name in sorted(expected)] + [directory / "identity.binding"]
        if any(stat.S_IMODE(path.stat().st_mode) & 0o077 for path in private):
            raise ValueError("installed onboarding left a private file readable by others")
        roles[role] = len(private)
    call("erebus-shielded-disclosure", {"method": "keygen", "key_file": str(root / "auditor.key")})
    if stat.S_IMODE((root / "auditor.key").stat().st_mode) & 0o077:
        raise ValueError("installed auditor key is readable by others")
    return {"suite_id": 2, "owner_only_private_files": roles, "bindings": 2, "auditor_key": True}


def qualify_offline_native(registry: Path, manifest: dict, uv: str, request: Path,
                           timeout_seconds: int = 600) -> dict:
    native = [name for name in manifest["wheels"] if name.startswith("erebus_cli-")]
    if len(native) != 1:
        raise ValueError("expected exactly one native wheel")
    with tempfile.TemporaryDirectory(prefix="metropolis-native-installed-") as temporary:
        root = Path(temporary).resolve()
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith(("EREBUS_", "UV_", "PIP_", "STARKNET_", "PROVING_"))
                       and key not in {"PYTHONPATH", "PYTHONHOME", "VIRTUAL_ENV"}}
        environment.update({"UV_CACHE_DIR": str(root / "cache"), "XDG_CONFIG_HOME": str(root / "config")})
        subprocess.run([uv, "venv", "--offline", "--python", sys.executable, str(root / "environment")],
                       env=environment, cwd=root, check=True)
        python = root / "environment/bin/python"
        subprocess.run([uv, "pip", "install", "--offline", "--no-deps", "--python", str(python), str(registry / "wheels" / native[0])],
                       env=environment, cwd=root, check=True)
        imports = subprocess.check_output([str(python), "-I", "-c",
            "import erebus_cli, importlib.metadata as m; print(erebus_cli.__file__); print(m.version('erebus-cli'))"],
            env=environment, cwd=root, text=True).splitlines()
        if len(imports) != 2 or not Path(imports[0]).resolve().is_relative_to(root / "environment") or imports[1] != manifest["version"]:
            raise ValueError("native package escaped the isolated install")
        bins = root / "environment/bin"
        for name, record in (manifest["binaries"] | manifest["launchers"]).items():
            if Path(name).name != name:
                raise ValueError("invalid binary inventory")
            with (bins / name).open("rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != record["sha256"]:
                    raise ValueError("installed native binary hash mismatch")
        environment["PATH"] = str(bins) + os.pathsep + "/usr/bin:/bin:/usr/sbin:/sbin"
        report = qualify_installed_prover(bins, manifest, request.resolve(), environment, timeout_seconds)
    return {"status": "verified", "scope": "native-prover-only", "version": manifest["version"],
            "dirty_source": manifest.get("dirty_source"), "build_profile": manifest.get("build_profile"),
            "source_commit": manifest.get("source_commit"), "evidence": evidence(manifest),
            "binaries": len(manifest["binaries"]), "source_imports": False,
            "live_payment": False, "published": False, "installed_proving": report,
            "installed_x402_rehearsal": False, "services_verified": False}


def check(registry: Path, rehearse_x402: bool = False, prover_request: Path | None = None,
          offline_native_prover: bool = False, prover_timeout_seconds: int = 600,
          rehearse_metered: bool = False, rehearse_assets: bool = False,
          proving_bundle: Path | None = None, proving_witnesses: Path | None = None,
          hosted_fixture: bool = False, hosted: list[dict] | dict | None = None,
          hosted_fixture_bundles: list[Path] | None = None, hosted_fixture_unrecorded: bool = False,
          hosted_witnesses: list[Path] | None = None, require_deployment: Path | None = None) -> dict:
    if type(prover_timeout_seconds) is not int or not 1 <= prover_timeout_seconds <= 3600:
        raise ValueError("qualification timeout must be between 1 and 3600 seconds")
    if proving_bundle is not None and proving_witnesses is None:
        raise ValueError("cold proving requires both a bundle and witness directory")
    if isinstance(hosted, dict):
        hosted = [hosted]
    fixture_bundles = list(hosted_fixture_bundles or ([proving_bundle] if proving_bundle is not None else []))
    if hosted_fixture_bundles and not hosted_fixture:
        raise ValueError("--hosted-fixture-bundle requires --hosted-fixture")
    if hosted_fixture and (not fixture_bundles or proving_witnesses is None):
        raise ValueError("the hosted fixture requires --proving-bundle and --proving-witnesses")
    if hosted and proving_witnesses is None:
        raise ValueError("hosted artifact qualification requires --proving-witnesses")
    if hosted and hosted_fixture:
        raise ValueError("choose either the hosted fixture or real hosted artifacts")
    domains = len(hosted) if hosted else len(fixture_bundles) if hosted_fixture else 0
    if hosted_witnesses and len(hosted_witnesses) != domains:
        raise ValueError("give one --hosted-witnesses directory per hosted domain, in the same order")
    witness_dirs = list(hosted_witnesses or [proving_witnesses] * domains)
    manifest = json.loads((registry / "release.json").read_text())
    if manifest["channel"] != "metropolis-testnet" or manifest["published"] is not False:
        raise ValueError("expected an unpublished Metropolis registry")
    coverage = None
    if require_deployment is not None:
        coverage = load_staging_module().deployment_coverage(require_deployment.resolve(), manifest)
        if not coverage["complete"]:
            raise ValueError("packages do not cover the deployment: " + "; ".join(coverage["problems"]))
    for name, record in manifest["wheels"].items():
        if Path(name).name != name:
            raise ValueError("invalid registry inventory")
        with (registry / "wheels" / name).open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != record["sha256"]:
                raise ValueError("registry wheel hash mismatch")
    uv = shutil.which("uv")
    if not uv:
        raise ValueError("uv is required")
    if offline_native_prover:
        if prover_request is None or rehearse_x402 or rehearse_metered or rehearse_assets or proving_bundle:
            raise ValueError("offline native qualification requires a prover request and excludes service rehearsals")
        return qualify_offline_native(registry, manifest, uv, prover_request, prover_timeout_seconds)
    prover_report = None
    cold_proving = None
    hosted_report = None
    with tempfile.TemporaryDirectory(prefix="metropolis-installed-") as temporary:
        # macOS temp dirs sit behind the /var -> /private/var symlink; compare resolved paths.
        root = Path(temporary).resolve()
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith(("EREBUS_", "UV_", "PIP_", "STARKNET_", "PROVING_"))
                       and key not in {"PYTHONPATH", "PYTHONHOME", "VIRTUAL_ENV"}}
        environment.update({"UV_CACHE_DIR": str(root / "uv-cache"), "XDG_CONFIG_HOME": str(root / "config")})
        subprocess.run([uv, "venv", "--python", sys.executable, str(root / "environment")],
                       env=environment, cwd=root, check=True)
        python = root / "environment/bin/python"
        server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(RegistryHandler, directory=str(registry)))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            subprocess.run([uv, "pip", "install", "--python", str(python),
                            "--extra-index-url", f"http://127.0.0.1:{server.server_port}/simple/",
                            "--index-url", "https://pypi.org/simple", "--index-strategy", "first-index",
                            f"erebus-mcp-server=={manifest['version']}"], env=environment, cwd=root, check=True)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
        bins = root / "environment/bin"
        environment["PATH"] = str(bins) + os.pathsep + "/usr/bin:/bin:/usr/sbin:/sbin"
        imports = subprocess.check_output([str(python), "-I", "-c",
            "import erebus, erebus_mcp, importlib.metadata as m; "
            "print(erebus.__file__); print(erebus_mcp.__file__); "
            "print(m.version('erebus-sdk')); print(m.version('erebus-cli')); print(m.version('erebus-mcp-server'))"],
            env=environment, cwd=root, text=True).splitlines()
        if any(not Path(path).resolve().is_relative_to(root / "environment") for path in imports[:2]) or imports[2:] != [manifest["version"]] * 3:
            raise ValueError("imports or package versions escaped the isolated environment")
        for name, record in (manifest["binaries"] | manifest["launchers"]).items():
            if Path(name).name != name:
                raise ValueError("invalid binary inventory")
            binary = bins / name
            with binary.open("rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != record["sha256"]:
                    raise ValueError("installed native binary hash mismatch")
        if prover_request is not None:
            prover_report = qualify_installed_prover(bins, manifest, prover_request.resolve(), environment, prover_timeout_seconds)
        if proving_bundle is not None:
            proving_root = root / "proving"
            proving_root.mkdir(mode=0o700)
            cold_proving = rehearse_cold_proving(bins, manifest, proving_bundle.resolve(), proving_witnesses.resolve(),
                                                 environment, proving_root, prover_timeout_seconds)
        if hosted_fixture:
            hosted_report = []
            for index, bundle in enumerate(fixture_bundles):
                hosted_root = root / f"hosted-fixture-{index}"
                hosted_root.mkdir(mode=0o700)
                hosted_report.append(rehearse_hosted_fixture(bins, manifest, bundle.resolve(), witness_dirs[index].resolve(),
                                                             environment, hosted_root, prover_timeout_seconds,
                                                             hosted_fixture_unrecorded))
        elif hosted:
            hosted_report = []
            for index, entry in enumerate(hosted):
                hosted_root = root / f"hosted-{index}"
                hosted_root.mkdir(mode=0o700)
                hosted_report.append(rehearse_hosted(bins, manifest, entry["manifest"].resolve(), entry["sha256"],
                                                     entry["publisher"], entry["signature"].resolve(),
                                                     witness_dirs[index].resolve(), environment, hosted_root,
                                                     prover_timeout_seconds))
        onboarding_root = root / "onboarding"
        onboarding_root.mkdir(mode=0o700)
        onboarding = rehearse_onboarding(bins, environment, onboarding_root)
        subprocess.run([str(bins / "erebus-selfhost"), "init", str(root / "operator files")],
                       env=environment, cwd=root, timeout=20, check=True)
        operator = root / "operator files"
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        relay_config = operator / "relay.env"
        relay_config.write_text(relay_config.read_text().replace("EREBUS_RELAY_PORT=8080", f"EREBUS_RELAY_PORT={port}"))
        try:
            for command in ("up", "check"):
                subprocess.run([str(bins / "erebus-selfhost"), command, str(operator)],
                               env=environment, cwd=root, timeout=30, check=True)
        finally:
            subprocess.run([str(bins / "erebus-selfhost"), "down", str(operator)],
                           env=environment, cwd=root, timeout=20, check=True)
        for name in ("erebus-negotiate", "erebus-payment"):
            result = subprocess.run([str(bins / name)], input='{"method":"version"}',
                                    text=True, capture_output=True, cwd=root, env=environment, timeout=20, check=True)
            response = json.loads(result.stdout)
            if response.get("protocol_version") != 1 or response.get("status") != "ok" or result.stderr:
                raise ValueError("installed native protocol mismatch")
            if name == "erebus-payment" and response.get("modes") != ["public-bound", "shielded"]:
                raise ValueError("installed payment modes are incomplete")
        private = root / "operator"
        private.mkdir(mode=0o700)
        key = private / "buyer.key"
        key.write_bytes(bytes(32))
        key.chmod(0o600)
        environment.update({
            "EREBUS_BACKEND": "access", "EREBUS_ACCESS_EVIDENCE_DIR": str(private),
            "EREBUS_ACCESS_BUYER_KEY_FILE": str(key), "EREBUS_ACCESS_SERVICE_URL": "https://unused.invalid/v1/access",
            "EREBUS_ACCESS_SERVICE_ID": "ab" * 32, "EREBUS_ACCESS_CACHE": str(private / "cache"),
        })
        code = """
import asyncio, json, os, sys
from mcp import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client
async def check():
    params = StdioServerParameters(command=sys.argv[1], env=dict(os.environ))
    async with stdio_client(params) as (reader, writer):
        async with ClientSession(reader, writer) as session:
            await session.initialize()
            names = {tool.name for tool in (await session.list_tools()).tools}
            tool, arguments, retry = sys.argv[2], json.loads(sys.argv[3]), sys.argv[4]
            expected = set(sys.argv[5].split(',')) if len(sys.argv) > 5 else {tool}
            assert names == expected, names
            result = await session.call_tool(tool, arguments)
            reply = result.structured_content or json.loads(result.content[0].text)
            assert reply['ok'] is False
            assert reply['error'][retry] is True
asyncio.run(check())
"""
        subprocess.run([str(python), "-I", "-c", code, str(bins / "erebus-mcp-server"), "retrieve_service_access",
                        json.dumps({"evidence_name": "../outside"}), "retry_without_payment"],
                       env=environment, cwd=root, timeout=45, check=True)
        if rehearse_metered:
            metered_environment = dict(environment)
            metered_environment.update({"EREBUS_ACCESS_PROFILE": "metered",
                                        "EREBUS_ACCESS_SERVICE_URL": "https://unused.invalid/v1/metered"})
            metered_tools = "call_metered_service,metered_service_status"
            for tool in metered_tools.split(","):
                subprocess.run([str(python), "-I", "-c", code, str(bins / "erebus-mcp-server"), tool,
                                json.dumps({"evidence_name": "../outside"}),
                                "retry_without_payment", metered_tools],
                               env=metered_environment, cwd=root, timeout=45, check=True)
        # The combined mode must resolve the installed native binaries from PATH, and a seller
        # server must expose negotiation only.
        negotiation = private / "negotiation.json"
        negotiation.write_text(json.dumps({"role": "seller", "state_root": str(private / "state")}))
        negotiation.chmod(0o600)
        for name in [key for key in environment if key.startswith("EREBUS_")]:
            del environment[name]
        environment.update({"EREBUS_BACKEND": "metropolis", "EREBUS_NEGOTIATION_CONFIG": str(negotiation)})
        subprocess.run([str(python), "-I", "-c", code, str(bins / "erebus-mcp-server"), "negotiate_deal",
                        json.dumps({"operation_ref": "../outside"}), "retry_without_new_payment"],
                       env=environment, cwd=root, timeout=45, check=True)
        if rehearse_x402 or rehearse_metered or rehearse_assets:
            checkout = Path(__file__).resolve().parents[1]
            cargo = shutil.which("cargo")
            if not cargo or not shutil.which("anvil"):
                raise ValueError("the local chain fixture requires cargo and Anvil")
            fixture_environment = dict(environment)
            fixture_environment.update({
                "PATH": str(bins) + os.pathsep + os.environ.get("PATH", ""),
                "EREBUS_TEST_INSTALLED_BIN_DIR": str(bins),
                "EREBUS_TEST_MCP_PYTHON": str(python),
            })
        if rehearse_metered:
            protocol = subprocess.run([str(bins / "erebus-access"), "--version"],
                                      env=environment, cwd=root, timeout=20, check=True,
                                      capture_output=True, text=True)
            if json.loads(protocol.stdout).get("protocol") != 2:
                raise ValueError("prepaid access requires the protocol-2 candidate binaries")
            subprocess.run([cargo, "test", "--locked", "--offline", "--manifest-path",
                            str(checkout / "sdk/shielded/Cargo.toml"), "--test", "negotiation_cli",
                            "funded_metered_access_serves_replays_and_recovers",
                            "--", "--ignored", "--nocapture"],
                           env=fixture_environment, cwd=root, timeout=900, check=True)
        if rehearse_assets:
            subprocess.run([cargo, "test", "--locked", "--offline", "--manifest-path",
                            str(checkout / "sdk/shielded/Cargo.toml"), "--test", "wallet_ops",
                            "fresh_operators", "--", "--ignored", "--nocapture", "--test-threads=1"],
                           env=fixture_environment, cwd=root, timeout=1800, check=True)
        if rehearse_x402:
            subprocess.run([cargo, "test", "--locked", "--offline", "--manifest-path",
                            str(checkout / "sdk/shielded/Cargo.toml"), "--test", "negotiation_cli",
                            "negotiated_x402_settles_once_recovers_restart_and_audits_from_the_grant",
                            "--", "--ignored", "--nocapture"],
                           env=fixture_environment, cwd=root, timeout=600, check=True)
            # Negotiation and the initial paid request through separate installed MCP servers.
            fixture_environment["EREBUS_TEST_MCP_SERVER"] = str(bins / "erebus-mcp-server")
            subprocess.run([cargo, "test", "--locked", "--offline", "--manifest-path",
                            str(checkout / "sdk/shielded/Cargo.toml"), "--test", "negotiation_cli",
                            "two_mcp_agents_pay_over_x402_through_a_dropped_response_and_restarts",
                            "--", "--ignored", "--nocapture"],
                           env=fixture_environment, cwd=root, timeout=900, check=True)
    if coverage is not None and hosted_report is not None:
        proved = {report.get("source_manifest_sha256") or report.get("manifest_sha256"): report.get("proved") is True
                  for report in hosted_report}
        for pool in coverage["pools"]:
            pool["proved"] = proved.get(pool["manifest_sha256"], False)
        missing = [pool["pool"] for pool in coverage["pools"] if not pool["proved"]]
        if missing:
            raise ValueError(f"installed proving did not cover every deployed pool: {missing}")
    transports = sorted({report.get("served_over") for report in [cold_proving, *(hosted_report or [])] if report})
    claims = {
        "platform": f"{platform.system()}-{platform.machine()}",
        "offline_proving": cold_proving is not None or bool(hosted_report),
        "proving_witnesses": "synthetic offline fixtures; no wallet, chain history or transaction" if cold_proving or hosted_report else None,
        "funded_payments": "local Anvil rehearsal with test keys" if rehearse_x402 or rehearse_metered or rehearse_assets else "none",
        "live_payment": False,
        "artifact_transport": transports,
        "https_exercised": any(report.get("https_exercised") is True for report in hosted_report or []),
        "publisher": sorted({report.get("publisher") for report in [cold_proving, *(hosted_report or [])] if report}),
        # Only a GitHub-hosted runner building a clean tree counts; local hosts and Rosetta containers never do.
        "native_platform_qualification": os.environ.get("GITHUB_ACTIONS") == "true"
        and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted" and manifest.get("dirty_source") is False,
        "note": "team rehearsal; not native CI, release, outside acceptance or live Monad evidence",
    }
    with (registry / "release.json").open("rb") as stream:
        release_sha256 = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"status": "verified", "version": manifest["version"], "binaries": len(manifest["binaries"]),
            "release_json_sha256": release_sha256, "claims": claims,
            "deployment_sha256": (coverage or {}).get("deployment_sha256"),
            "evidence": evidence(manifest), "fixture_harness_from_checkout": rehearse_x402 or rehearse_metered or rehearse_assets,
            "source_imports": False, "live_payment": False, "published": False,
            "installed_x402_rehearsal": rehearse_x402,
            "installed_metered_rehearsal": rehearse_metered,
            "installed_asset_rehearsal": rehearse_assets,
            "installed_proving": prover_report, "installed_cold_proving": cold_proving,
            "installed_onboarding": onboarding, "installed_hosted_artifacts": hosted_report,
            "deployment_coverage": coverage}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--rehearse-x402", action="store_true", help="run the Anvil fixture with installed participant, access, MCP, and auditor binaries")
    parser.add_argument("--rehearse-metered", action="store_true", help="run the candidate-v2 Anvil prepaid fixture with installed participant, access, and auditor binaries; test setup only")
    parser.add_argument("--rehearse-assets", action="store_true", help="run random-key ERC-20 and wrapped-native private payment workflows from installed binaries; Anvil and test setup only")
    parser.add_argument("--prover-request", type=Path, help="qualify the installed prover with an owner-only request and pinned artifacts; reports fixture status explicitly")
    parser.add_argument("--offline-native-prover", action="store_true", help="install only the hash-checked native wheel offline; does not verify SDK/MCP or services")
    parser.add_argument("--proving-bundle", type=Path, help="test-only artifact bundle whose manifest the release recorded; proves all transitions from empty caches over loopback")
    parser.add_argument("--proving-witnesses", type=Path, help="directory of <circuit>-input.json fixture witnesses for --proving-bundle")
    parser.add_argument("--hosted-fixture", action="store_true", help="exercise the hosted-download path against a local redirecting release server built from --proving-bundle")
    parser.add_argument("--hosted-fixture-bundle", type=Path, action="append", default=[], help="repeatable: test-only bundle per pool domain for --hosted-fixture (default: --proving-bundle)")
    parser.add_argument("--hosted-fixture-unrecorded", action="store_true", help="allow --hosted-fixture-bundle domains the release did not record; reported as release_recorded=false, never qualification")
    parser.add_argument("--hosted-witnesses", type=Path, action="append", default=[], help="repeatable, in hosted-domain order: fixture witnesses bound to that domain (default: --proving-witnesses for every domain)")
    parser.add_argument("--require-deployment", type=Path, help="deployment record: packages must record every pool, and hosted runs must prove every pool")
    parser.add_argument("--hosted-manifest", type=Path, action="append", default=[], help="repeatable, one per pool domain: real hosted manifest (HTTPS URLs) that the release recorded")
    parser.add_argument("--hosted-manifest-sha256", action="append", default=[], help="repeatable, in --hosted-manifest order: independently pinned SHA-256")
    parser.add_argument("--hosted-publisher", help="independently pinned publisher address for every --hosted-manifest")
    parser.add_argument("--hosted-signature", type=Path, action="append", default=[], help="repeatable, in --hosted-manifest order: detached signature envelope")
    parser.add_argument("--prover-timeout-seconds", type=int, default=600, help="bounded qualification deadline, 1-3600 seconds; does not change product or agreement timeouts")
    args = parser.parse_args()
    counts = {len(args.hosted_manifest), len(args.hosted_manifest_sha256), len(args.hosted_signature)}
    if (args.hosted_manifest or args.hosted_publisher) and (len(counts) != 1 or args.hosted_publisher is None):
        raise SystemExit("hosted qualification needs --hosted-publisher and, per domain, --hosted-manifest, --hosted-manifest-sha256 and --hosted-signature")
    hosted = [{"manifest": manifest, "sha256": pin, "publisher": args.hosted_publisher, "signature": signature}
              for manifest, pin, signature in zip(args.hosted_manifest, args.hosted_manifest_sha256, args.hosted_signature)] or None
    print(json.dumps(check(args.registry.resolve(), args.rehearse_x402, args.prover_request, args.offline_native_prover,
                           args.prover_timeout_seconds, args.rehearse_metered, args.rehearse_assets,
                           args.proving_bundle, args.proving_witnesses, args.hosted_fixture, hosted,
                           args.hosted_fixture_bundle, args.hosted_fixture_unrecorded,
                           args.hosted_witnesses, args.require_deployment)))


if __name__ == "__main__":
    main()
