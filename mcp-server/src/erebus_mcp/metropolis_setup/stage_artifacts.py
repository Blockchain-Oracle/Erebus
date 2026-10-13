#!/usr/bin/env python3
"""Stage a proving-artifact bundle for HTTPS hosting, or verify a hosted copy. Never uploads.

    stage_artifacts.py check-bundle --bundle DIR --manifest-sha256 PIN --deployment FILE [--pool NAME]
                                    [--source-manifest FILE --source-manifest-sha256 PIN]
    stage_artifacts.py stage  --bundle DIR --manifest-sha256 PIN --base-url URL --output NEW_DIR [--domain-name NAME]
    stage_artifacts.py verify --staged DIR
    stage_artifacts.py fetch  --manifest FILE --manifest-sha256 PIN --output NEW_DIR
    stage_artifacts.py coverage --deployment FILE --release release.json
    stage_artifacts.py preflight --staged DIR --publisher ADDR --signature FILE --erebus-artifacts BIN
                                 (--deployment FILE [--pool NAME] | --rehearsal-domain ADDR) [--release release.json ...]

`check-bundle` compares a domain-bound bundle with one pool of a deployment record before staging: the
pinned manifest and every file, chain, pool address, verifier version, each circuit's verifier runtime
against the deployment's runtime pins, and the verification-key digests. With a source manifest it also
requires that only the domain changed, and that the source is the manifest the deployment recorded.

`stage` checks the bundle against its independently pinned manifest digest, then writes flat
release-asset names (GitHub release assets cannot hold directories) and a hosting manifest whose
URLs name `<base-url>/<file>`. Several pool domains may share one release base and the same artifact
bytes; `--domain-name` gives each domain's manifest and signature their own asset names. The three
public verification keys are staged beside the artifacts (`<circuit>-verification-key.json`), checked
against each circuit's `verification_key_sha256`, so a fresh runner can rebuild the bundle layout.

`fetch` downloads a hosted manifest's nine files and three verification keys over HTTPS into a new
directory in bundle layout, checking every size and digest. It is hosted-download evidence only when the
URLs are the real release; it never uploads.

`coverage` reports whether a package `release.json` recorded exactly one proving manifest for every pool of
a deployment record (chain, pool address, verifier version, verifier runtime pins), and nothing else. Every file identity, the domain, `test_only`, and the per-circuit
verifier identities are unchanged; only the URLs differ. The hosting manifest therefore has a new
digest. It must be reviewed, signed with `erebus-artifacts --sign-manifest` using an offline
publisher key, and pinned independently, like any manifest.

`verify` downloads every URL in a staged hosting manifest over HTTPS and compares size and SHA-256.
It is the only step that can establish that files are actually hosted; staging alone is not
hosting evidence, and neither is a placeholder URL.

`preflight` runs before any upload and never touches the network. It refuses placeholder or non-HTTPS
URLs, a missing or invalid publisher signature (checked by the installed `erebus-artifacts` against a
pre-filled cache), a domain that matches neither the supplied deployment configuration nor an explicitly
declared rehearsal domain, verifier runtimes that differ from the deployment's pins, and package
manifests that did not record this proving manifest. Results always carry `test-only` (when the
manifest says so) and `unaudited`, and `single-operator` unless the deployment record declares
independent operators; nothing here can remove those labels.
"""

from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path

KINDS = ("deposit", "transfer", "withdraw")
FIELDS = ("wasm", "r1cs", "zkey")
HEX64 = re.compile(r"[0-9a-f]{64}", re.ASCII)
PLACEHOLDER_SUFFIXES = (".invalid", ".example", ".test", ".localhost", ".local")
MAX_FILE_BYTES = 256 * 1024 * 1024
DOMAIN_NAME = re.compile(r"[a-z0-9][a-z0-9-]{0,31}", re.ASCII)


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def bundle_path(kind: str, field: str) -> str:
    return f"{kind}_js/{kind}.wasm" if field == "wasm" else f"{kind}.{field}"


def require_hostable_base_url(base_url: str) -> str:
    """Refuse URLs that cannot be real public hosting: placeholders, loopback, credentials, plain HTTP."""
    url = urllib.parse.urlsplit(base_url)
    host = (url.hostname or "").lower()
    if url.scheme != "https" or not host or url.username or url.password or url.query or url.fragment:
        raise ValueError("base URL must be plain https without credentials, query, or fragment")
    if host in {"localhost", "example.com", "example.org", "example.net"} or host.endswith(PLACEHOLDER_SUFFIXES):
        raise ValueError("base URL names a placeholder or local host")
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        address = None
    if address is not None and (address.is_loopback or address.is_private or address.is_link_local):
        raise ValueError("base URL names a non-public address")
    return base_url.rstrip("/") + "/"


def stage(bundle: Path, pin: str, base_url: str, output: Path, domain_name: str | None = None) -> dict:
    if not HEX64.fullmatch(pin):
        raise ValueError("manifest pin must be 64 lowercase hex characters")
    if domain_name is not None and not DOMAIN_NAME.fullmatch(domain_name):
        raise ValueError("domain name must be 1-32 lowercase letters, digits, or hyphens")
    base = require_hostable_base_url(base_url)
    if output.exists():
        raise ValueError("use a new output directory")
    source_bytes = (bundle / "manifest.json").read_bytes()
    if hashlib.sha256(source_bytes).hexdigest() != pin:
        raise ValueError("bundle manifest does not match its pin")
    source = json.loads(source_bytes)
    if sorted(circuit["kind"] for circuit in source["circuits"]) != sorted(KINDS):
        raise ValueError("bundle must name exactly the deposit, transfer, and withdraw circuits")
    hosting = json.loads(source_bytes)
    files = []
    for circuit in hosting["circuits"]:
        for field in FIELDS:
            record = circuit[field]
            local = bundle / bundle_path(circuit["kind"], field)
            if local.is_symlink() or not local.is_file() or local.stat().st_size != record["bytes"] \
                    or record["bytes"] > MAX_FILE_BYTES or sha256(local) != record["sha256"]:
                raise ValueError(f"bundle file does not match the manifest: {bundle_path(circuit['kind'], field)}")
            name = f"{circuit['kind']}.{field}"
            record["url"] = base + name
            files.append((name, local, record["sha256"], record["bytes"]))
    keys = []
    for circuit in source["circuits"]:
        key = bundle / f"{circuit['kind']}-verification-key.json"
        expected = circuit.get("identity", {}).get("verification_key_sha256")
        if expected is not None:
            if key.is_symlink() or not key.is_file() or sha256(key) != expected:
                raise ValueError(f"verification key does not match the manifest identity: {key.name}")
            keys.append((key.name, key, expected, key.stat().st_size))
    for original, rewritten in zip(source["circuits"], hosting["circuits"]):
        if {k: v for k, v in original.items() if k not in FIELDS} != {k: v for k, v in rewritten.items() if k not in FIELDS}:
            raise ValueError("staging changed a circuit identity")
    if {k: v for k, v in source.items() if k != "circuits"} != {k: v for k, v in hosting.items() if k != "circuits"}:
        raise ValueError("staging changed the manifest domain")
    (output / "files").mkdir(parents=True)
    for name, local, digest, _ in files + keys:
        shutil.copyfile(local, output / "files" / name)
        if sha256(output / "files" / name) != digest:
            raise ValueError("copied artifact identity mismatch")
    manifest_bytes = (json.dumps(hosting, indent=2) + "\n").encode()
    (output / "manifest.json").write_bytes(manifest_bytes)
    (output / "SHA256SUMS").write_text("".join(f"{digest}  {name}\n" for name, _, digest, _ in sorted(files + keys)))
    record = {
        "source_manifest_sha256": pin,
        "hosting_manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "base_url": base,
        "test_only": source["test_only"],
        "chain_id": source.get("chain_id"),
        "settlement_contract": source.get("settlement_contract"),
        "verifier_version": source.get("verifier_version"),
        "files": [{"name": name, "sha256": digest, "bytes": size} for name, _, digest, size in sorted(files)],
        "verification_keys": [{"name": name, "sha256": digest, "bytes": size} for name, _, digest, size in sorted(keys)],
        "domain_name": domain_name,
        "manifest_asset": f"manifest-{domain_name}.json" if domain_name else "manifest.json",
        "signature_asset": f"manifest-{domain_name}.signature.json" if domain_name else "manifest.signature.json",
        "signed": False,
        "hosted": False,
    }
    (output / "hosting.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def _request(url: str) -> urllib.request.Request:
    """Hosted downloads carry an explicit User-Agent; some hosts and RPC providers reject library defaults."""
    return urllib.request.Request(url, headers={"user-agent": "erebus-metropolis-packaging"})


def verify(staged: Path, opener=urllib.request.urlopen) -> dict:
    manifest = json.loads((staged / "manifest.json").read_bytes())
    results = []
    for circuit in manifest["circuits"]:
        for field in FIELDS:
            record = circuit[field]
            require_hostable_base_url(record["url"].rsplit("/", 1)[0] + "/")
            digest, size = hashlib.sha256(), 0
            with opener(_request(record["url"]), timeout=60) as response:
                while chunk := response.read(1 << 20):
                    size += len(chunk)
                    if size > record["bytes"]:
                        break
                    digest.update(chunk)
            ok = size == record["bytes"] and digest.hexdigest() == record["sha256"]
            results.append({"url": record["url"], "ok": ok})
    return {"hosted": all(result["ok"] for result in results), "files": results,
            "manifest_sha256": sha256(staged / "manifest.json")}


def download(url: str, limit: int, opener=urllib.request.urlopen) -> bytes:
    require_hostable_base_url(url.rsplit("/", 1)[0] + "/")
    chunks, size = [], 0
    with opener(_request(url), timeout=60) as response:
        while chunk := response.read(1 << 20):
            size += len(chunk)
            if size > limit:
                raise ValueError(f"download exceeds its declared size: {url}")
            chunks.append(chunk)
    return b"".join(chunks)


def fetch(manifest_file: Path, pin: str, output: Path, opener=urllib.request.urlopen) -> dict:
    """Rebuild a bundle directory from hosted assets: the nine artifacts plus the verification keys."""
    data = manifest_file.read_bytes()
    if not HEX64.fullmatch(pin) or hashlib.sha256(data).hexdigest() != pin:
        raise ValueError("hosted manifest does not match its pin")
    if output.exists():
        raise ValueError("use a new output directory")
    manifest = json.loads(data)
    bases = {circuit[field]["url"].rsplit("/", 1)[0] + "/" for circuit in manifest["circuits"] for field in FIELDS}
    if len(bases) != 1:
        raise ValueError("artifact URLs must share one release base")
    base = bases.pop()
    output.mkdir(parents=True)
    fetched = []
    for circuit in manifest["circuits"]:
        kind = circuit["kind"]
        for field in FIELDS:
            record = circuit[field]
            body = download(record["url"], record["bytes"], opener)
            if len(body) != record["bytes"] or hashlib.sha256(body).hexdigest() != record["sha256"]:
                raise ValueError(f"hosted artifact differs from the manifest: {record['url']}")
            target = output / bundle_path(kind, field)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(body)
            fetched.append({"url": record["url"], "sha256": record["sha256"], "bytes": record["bytes"]})
        url = base + f"{kind}-verification-key.json"
        body = download(url, 1 << 20, opener)
        if hashlib.sha256(body).hexdigest() != circuit["identity"]["verification_key_sha256"]:
            raise ValueError(f"hosted verification key differs from the manifest identity: {url}")
        (output / f"{kind}-verification-key.json").write_bytes(body)
        fetched.append({"url": url, "sha256": hashlib.sha256(body).hexdigest(), "bytes": len(body)})
    (output / "manifest.json").write_bytes(data)
    return {"manifest_sha256": pin, "settlement_contract": manifest.get("settlement_contract"),
            "test_only": manifest.get("test_only"), "files": fetched, "hosted": True}


ADDRESS = re.compile(r"0x[0-9a-f]{40}", re.ASCII)


def deployment_identity(path: Path, pool_name: str | None = None) -> dict:
    """Read only the deployment fields the wallet configuration pins (`namespace`, `pool`,
    `verifier_version`, `runtime_pins`). A full wallet configuration is accepted, and so is a
    deployment record with a `pools` list, from which `pool_name` selects exactly one entry."""
    record = json.loads(path.read_text())
    config = record
    if "pools" in record:
        matches = [entry for entry in record["pools"] if pool_name is not None and entry.get("name") == pool_name]
        if len(matches) != 1:
            raise ValueError("a deployment record with several pools requires --pool naming exactly one")
        config = matches[0]
        if record.get("chain_id") is not None and f"eip155:{record['chain_id']}" != config.get("namespace"):
            raise ValueError("deployment pool namespace differs from the record's chain")
    elif pool_name is not None:
        raise ValueError("--pool applies only to a deployment record with a pools list")
    namespace = config.get("namespace", "")
    if not re.fullmatch(r"eip155:[1-9][0-9]*", namespace, re.ASCII):
        raise ValueError("deployment namespace must be eip155:<chain id>")
    pool = str(config.get("pool", "")).lower()
    pins = config.get("runtime_pins")
    if not ADDRESS.fullmatch(pool) or pool == "0x" + "0" * 40 or type(config.get("verifier_version")) is not int:
        raise ValueError("deployment requires a pool address and verifier version")
    if not isinstance(pins, dict) or set(pins) != {"pool", "poseidon", "deposit", "transfer", "withdraw"}:
        raise ValueError("deployment requires the five runtime pins")
    normalized = {}
    for name, value in pins.items():
        digits = str(value).removeprefix("0x")
        if not HEX64.fullmatch(digits) or digits == "0" * 64:
            raise ValueError("runtime pins must be non-zero 32-byte lowercase hex")
        normalized[name] = digits
    return {"chain_id": int(namespace.split(":")[1]), "settlement_contract": pool,
            "verifier_version": config["verifier_version"], "runtime_pins": normalized,
            "test_only": record.get("test_only"), "independent": record.get("independent") is True,
            "manifest_sha256": record.get("manifest_sha256")}


def check_bundle(bundle: Path, pin: str, deployment: Path, pool_name: str | None,
                 source_manifest: Path | None = None, source_pin: str | None = None) -> dict:
    """Compare a domain-bound bundle with one deployed pool. Offline; reads no chain state."""
    problems: list[str] = []
    data = (bundle / "manifest.json").read_bytes()
    if not HEX64.fullmatch(pin) or hashlib.sha256(data).hexdigest() != pin:
        raise ValueError("bundle manifest does not match its pin")
    manifest = json.loads(data)
    identity = deployment_identity(deployment, pool_name)
    for key in ("chain_id", "settlement_contract", "verifier_version"):
        if str(manifest.get(key)).lower() != str(identity[key]).lower():
            problems.append(f"manifest {key} does not match the deployment")
    if identity["test_only"] is True and manifest.get("test_only") is not True:
        problems.append("a test-only deployment requires a test-only manifest")
    if sorted(circuit["kind"] for circuit in manifest["circuits"]) != sorted(KINDS):
        problems.append("bundle must name exactly the deposit, transfer, and withdraw circuits")
    for circuit in manifest["circuits"]:
        kind = circuit["kind"]
        recorded = circuit.get("identity", {})
        if recorded.get("verifier_runtime_keccak256") != identity["runtime_pins"].get(kind):
            problems.append(f"{kind} verifier runtime does not match the deployment pin")
        key = bundle / f"{kind}-verification-key.json"
        if not key.is_file() or sha256(key) != recorded.get("verification_key_sha256"):
            problems.append(f"{kind} verification key differs from the manifest identity")
        for field in FIELDS:
            local = bundle / bundle_path(kind, field)
            if local.is_symlink() or not local.is_file() or local.stat().st_size != circuit[field]["bytes"] \
                    or sha256(local) != circuit[field]["sha256"]:
                problems.append(f"bundle file does not match the manifest: {bundle_path(kind, field)}")
    source_report = None
    if (source_manifest is None) != (source_pin is None):
        raise ValueError("a source manifest needs its pin")
    if source_manifest is not None:
        source_bytes = source_manifest.read_bytes()
        if hashlib.sha256(source_bytes).hexdigest() != source_pin:
            raise ValueError("source manifest does not match its pin")
        if identity["manifest_sha256"] is not None and source_pin != identity["manifest_sha256"]:
            problems.append("source manifest is not the manifest the deployment recorded")
        source = json.loads(source_bytes)
        changed = sorted(key for key in set(source) | set(manifest)
                         if key != "circuits" and source.get(key) != manifest.get(key))
        if [key for key in changed if key != "settlement_contract"]:
            problems.append(f"rebinding changed more than the domain: {changed}")
        strip = lambda circuit: {k: ({f: v for f, v in val.items() if f != "url"} if k in FIELDS else val)
                                 for k, val in circuit.items()}
        if [strip(c) for c in source["circuits"]] != [strip(c) for c in manifest["circuits"]]:
            problems.append("rebinding changed an artifact or circuit identity")
        source_report = {"source_manifest_sha256": source_pin, "changed_fields": changed}
    return {"bundle_manifest_sha256": pin, "pool": pool_name, "chain_id": manifest.get("chain_id"),
            "settlement_contract": manifest.get("settlement_contract"),
            "verifier_version": manifest.get("verifier_version"), "test_only": manifest.get("test_only"),
            "runtime_pins_checked": sorted(KINDS), "source": source_report, "problems": problems,
            "matches_deployment": not problems,
            "note": "pool, poseidon and asset runtime pins are deployment facts not carried by the bundle"}


def verify_signature(binary: Path, staged: Path, manifest: dict, authorization: dict) -> list[str]:
    """Run the installed installer offline: every file is pre-cached, so only authorization and
    cached-byte verification can fail."""
    problems = []
    with tempfile.TemporaryDirectory(prefix="metropolis-preflight-") as temporary:
        cache = Path(temporary) / "cache"
        cache.mkdir(mode=0o700)
        for circuit in manifest["circuits"]:
            for field in FIELDS:
                shutil.copyfile(staged / "files" / f"{circuit['kind']}.{field}", cache / f"{circuit[field]['sha256']}.artifact")
        digest = sha256(staged / "manifest.json")
        for kind in KINDS:
            request = {"manifest_file": str(staged / "manifest.json"), "manifest_sha256": digest, "circuit": kind,
                       "cache_root": str(cache), "manifest_authorization": authorization,
                       "allow_test_artifacts": manifest["test_only"]}
            result = subprocess.run([str(binary)], input=json.dumps(request), text=True, capture_output=True,
                                    timeout=60, check=False)
            try:
                reply = json.loads(result.stdout)
            except ValueError:
                reply = {}
            if result.returncode != 0 or reply.get("status") != "ok" or reply.get("downloaded_bytes", 1) != 0:
                problems.append(f"publisher signature or cached artifacts rejected for {kind}")
    return problems


def deployment_coverage(deployment: Path, release: dict) -> dict:
    """Whether the packages recorded exactly one proving manifest per deployed pool, and nothing else."""
    record = json.loads(deployment.read_text())
    if "pools" not in record:
        raise ValueError("coverage needs a deployment record with a pools list")
    domains = release.get("proving_domains") or ([release["proving_artifacts"]] if release.get("proving_artifacts") else [])
    problems, pools = [], []
    for entry in record["pools"]:
        identity = deployment_identity(deployment, entry.get("name"))
        matches = [domain for domain in domains
                   if domain.get("chain_id") == identity["chain_id"]
                   and str(domain.get("settlement_contract")).lower() == identity["settlement_contract"]]
        if len(matches) != 1:
            problems.append(f"{entry.get('name')}: packages recorded {len(matches)} manifests for this pool")
            pools.append({"pool": entry.get("name"), "manifest_sha256": None})
            continue
        domain = matches[0]
        if domain.get("verifier_version") != identity["verifier_version"]:
            problems.append(f"{entry.get('name')}: recorded verifier version differs from the deployment")
        for kind in KINDS:
            runtime = ((domain.get("circuits") or {}).get(kind) or {}).get("identity", {}).get("verifier_runtime_keccak256")
            if runtime != identity["runtime_pins"][kind]:
                problems.append(f"{entry.get('name')}: recorded {kind} verifier runtime differs from the deployment pin")
        if identity["test_only"] is True and domain.get("test_only") is not True:
            problems.append(f"{entry.get('name')}: a test-only deployment requires a test-only manifest")
        pools.append({"pool": entry.get("name"), "settlement_contract": identity["settlement_contract"],
                      "manifest_sha256": domain.get("manifest_sha256")})
    deployed = {(entry["chain_id"], entry["settlement_contract"]) for entry in
                (deployment_identity(deployment, pool.get("name")) for pool in record["pools"])}
    extra = [domain.get("manifest_sha256") for domain in domains
             if (domain.get("chain_id"), str(domain.get("settlement_contract")).lower()) not in deployed]
    if extra:
        problems.append(f"packages recorded proving manifests outside the deployment: {extra}")
    return {"deployment_sha256": sha256(deployment), "pools": pools, "problems": problems, "complete": not problems}


def recorded_manifests(release: dict) -> set[str]:
    """Every proving manifest a package release recorded (one per pool domain)."""
    records = release.get("proving_domains") or ([release["proving_artifacts"]] if release.get("proving_artifacts") else [])
    return {record.get("manifest_sha256") for record in records}


def preflight(staged: Path, publisher: str, signature: Path, binary: Path, deployment: Path | None,
              rehearsal_domain: str | None, releases: list[Path], pool_name: str | None = None) -> dict:
    problems: list[str] = []
    record = json.loads((staged / "hosting.json").read_text())
    manifest_bytes = (staged / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()
    if manifest_sha256 != record["hosting_manifest_sha256"]:
        problems.append("hosting manifest changed after staging")
    if type(manifest.get("test_only")) is not bool:
        problems.append("manifest does not declare its fixture boundary")
    bases = set()
    for circuit in manifest["circuits"]:
        for field in FIELDS:
            url = circuit[field]["url"]
            base = url.rsplit("/", 1)[0] + "/"
            try:
                require_hostable_base_url(base)
            except ValueError as error:
                problems.append(f"{circuit['kind']}.{field}: {error}")
            bases.add(base)
            local = staged / "files" / f"{circuit['kind']}.{field}"
            if not local.is_file() or local.stat().st_size != circuit[field]["bytes"] or sha256(local) != circuit[field]["sha256"]:
                problems.append(f"staged file differs from the manifest: {local.name}")
    if len(bases) != 1:
        problems.append("artifact URLs must share one release base")
    if not ADDRESS.fullmatch(publisher) or publisher == "0x" + "0" * 40:
        problems.append("publisher must be an independently pinned lowercase address")
    if not signature.is_file():
        problems.append("detached publisher signature is missing")
    elif not binary.is_file():
        problems.append("signature verification needs the installed erebus-artifacts binary")
    elif not problems:
        problems.extend(verify_signature(binary, staged, manifest,
                                         {"publisher": publisher, "signature": json.loads(signature.read_text())}))
    if (deployment is None) == (rehearsal_domain is None):
        problems.append("name exactly one of an authenticated deployment configuration or a rehearsal domain")
        domain = "unknown"
    elif deployment is not None:
        domain = "deployment"
        identity = deployment_identity(deployment, pool_name)
        independent = identity["independent"]
        if identity["test_only"] is True and manifest.get("test_only") is not True:
            problems.append("a test-only deployment requires a test-only manifest")
        for key in ("chain_id", "settlement_contract", "verifier_version"):
            if str(manifest.get(key)).lower() != str(identity[key]).lower():
                problems.append(f"manifest {key} does not match the deployment")
        for circuit in manifest["circuits"]:
            runtime = circuit.get("identity", {}).get("verifier_runtime_keccak256")
            if runtime != identity["runtime_pins"][circuit["kind"]]:
                problems.append(f"{circuit['kind']} verifier runtime does not match the deployment pin")
    else:
        domain = "rehearsal"
        independent = False
        if str(manifest.get("settlement_contract")).lower() != rehearsal_domain.lower():
            problems.append("manifest domain is not the declared rehearsal domain")
        if manifest.get("test_only") is not True:
            problems.append("a rehearsal domain is only valid for a test-only manifest")
    packages = []
    for path in releases:
        release = json.loads(path.read_text())
        records = release.get("proving_domains") or [release.get("proving_artifacts") or {}]
        recorded = next((record for record in records if record.get("manifest_sha256") == manifest_sha256), {})
        eligible = bool((release.get("candidate") or {}).get("release_eligible"))
        if manifest_sha256 not in recorded_manifests(release):
            problems.append(f"{path.name}: packages did not record this proving manifest")
        elif recorded.get("test_only") is not manifest.get("test_only"):
            problems.append(f"{path.name}: packages disagree about the fixture boundary")
        packages.append({"release": path.name, "platform": release.get("platform"), "release_eligible": eligible})
    labels = (["test-only"] if manifest.get("test_only") else []) + ["unaudited"]
    if domain == "unknown" or not independent:
        labels.append("single-operator")
    if domain == "rehearsal":
        labels.append("rehearsal-domain")
    return {
        "manifest_sha256": manifest_sha256, "publisher": publisher, "domain": domain, "pool": pool_name,
        "settlement_contract": manifest.get("settlement_contract"), "labels": labels,
        "problems": problems, "artifacts_publishable": not problems,
        "packages_publishable": not problems and bool(packages) and all(item["release_eligible"] for item in packages),
        "packages": packages, "uploaded": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    staging = commands.add_parser("stage")
    staging.add_argument("--bundle", type=Path, required=True)
    staging.add_argument("--manifest-sha256", required=True)
    staging.add_argument("--base-url", required=True)
    staging.add_argument("--output", type=Path, required=True)
    staging.add_argument("--domain-name", help="pool name for this domain's manifest and signature asset names")
    bundle_check = commands.add_parser("check-bundle")
    bundle_check.add_argument("--bundle", type=Path, required=True)
    bundle_check.add_argument("--manifest-sha256", required=True)
    bundle_check.add_argument("--deployment", type=Path, required=True)
    bundle_check.add_argument("--pool")
    bundle_check.add_argument("--source-manifest", type=Path)
    bundle_check.add_argument("--source-manifest-sha256")
    fetching = commands.add_parser("fetch")
    fetching.add_argument("--manifest", type=Path, required=True)
    fetching.add_argument("--manifest-sha256", required=True)
    fetching.add_argument("--output", type=Path, required=True)
    covering = commands.add_parser("coverage")
    covering.add_argument("--deployment", type=Path, required=True)
    covering.add_argument("--release", type=Path, required=True)
    checking = commands.add_parser("verify")
    checking.add_argument("--staged", type=Path, required=True)
    gate = commands.add_parser("preflight")
    gate.add_argument("--staged", type=Path, required=True)
    gate.add_argument("--publisher", required=True)
    gate.add_argument("--signature", type=Path, required=True)
    gate.add_argument("--erebus-artifacts", type=Path, required=True)
    gate.add_argument("--deployment", type=Path)
    gate.add_argument("--pool", help="pool name when --deployment is a record with several pools")
    gate.add_argument("--rehearsal-domain")
    gate.add_argument("--release", type=Path, action="append", default=[])
    args = parser.parse_args()
    try:
        if args.command == "stage":
            result = stage(args.bundle.resolve(), args.manifest_sha256, args.base_url, args.output.resolve(),
                           args.domain_name)
        elif args.command == "check-bundle":
            result = check_bundle(args.bundle.resolve(), args.manifest_sha256, args.deployment.resolve(), args.pool,
                                  args.source_manifest.resolve() if args.source_manifest else None,
                                  args.source_manifest_sha256)
            print(json.dumps({"status": "ok" if result["matches_deployment"] else "refused", **result}, indent=2))
            return 0 if result["matches_deployment"] else 1
        elif args.command == "fetch":
            result = fetch(args.manifest.resolve(), args.manifest_sha256, args.output.resolve())
        elif args.command == "coverage":
            result = deployment_coverage(args.deployment.resolve(), json.loads(args.release.read_text()))
            print(json.dumps({"status": "ok" if result["complete"] else "refused", **result}, indent=2))
            return 0 if result["complete"] else 1
        elif args.command == "verify":
            result = verify(args.staged.resolve())
        else:
            result = preflight(args.staged.resolve(), args.publisher, args.signature.resolve(),
                               args.erebus_artifacts.resolve(), args.deployment, args.rehearsal_domain,
                               [path.resolve() for path in args.release], args.pool)
            print(json.dumps({"status": "ok" if result["artifacts_publishable"] else "refused", **result}, indent=2))
            return 0 if result["artifacts_publishable"] else 1
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({"status": "error", "error": str(error) if isinstance(error, ValueError) else type(error).__name__}))
        return 2
    print(json.dumps({"status": "ok", **result}, indent=2))
    return 0 if result.get("hosted", True) else 1


if __name__ == "__main__":
    sys.exit(main())
