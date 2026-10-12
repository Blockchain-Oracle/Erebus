#!/usr/bin/env python3
"""Install a published Metropolis release from the public index and prove both pools over public HTTPS.

    check-metropolis-public-install.py --version V --authorization testnet-release.json
        --witnesses wmon=DIR --witnesses usdc=DIR [--work NEW_DIR]

Nothing here builds, signs, uploads or sends a transaction. In a new directory with a new uv cache it:
1. installs `erebus-mcp-server==V` from the public index (PyPI only supplies missing dependencies);
2. downloads the release's `release-manifest.json` over HTTPS and requires every installed command for this
   platform to match it byte for byte, and the imports to resolve inside the new environment;
3. for each pool in the authorization, downloads the hosting manifest and its detached signature over
   HTTPS, requires the manifest digest the authorization pins, and proves deposit, transfer and withdrawal
   with the installed `erebus-local-prove` from an empty cache, authorized by the pinned publisher;
4. requires a wrong publisher to be refused before anything installs.

Witness directories hold synthetic offline `<circuit>-input.json` fixtures bound to each pool's domain
(`circuits/m5/scripts/qualification-witnesses.mjs`). This is install and proving evidence, not a payment.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

INDEX = "https://poulavbhowmick03.github.io/erebus-metropolis/simple/"
RELEASES = "https://github.com/PoulavBhowmick03/erebus-metropolis/releases/download"
TRANSITIONS = {
    "deposit": ["chainId", "contractAddress", "verifierVersion", "asset", "amount", "noteCommitment"],
    "transfer": ["chainId", "contractAddress", "verifierVersion", "asset", "dealCommitment", "dealNullifier", "root",
                 "inputNullifier", "paymentCommitment", "changeCommitment", "expiry"],
    "withdraw": ["chainId", "contractAddress", "verifierVersion", "asset", "root", "noteNullifier", "recipient", "amount"],
}


def https(url: str, limit: int = 1 << 20) -> bytes:
    if not url.startswith("https://"):
        raise ValueError("public downloads must use https")
    with urllib.request.urlopen(url, timeout=120) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"download exceeds {limit} bytes: {url}")
    return data


def wheel_tag() -> str:
    tags = {("Darwin", "arm64"): "macosx_11_0_arm64", ("Linux", "x86_64"): "linux_x86_64"}
    tag = tags.get((platform.system(), platform.machine()))
    if tag is None:
        raise ValueError("published releases support macOS arm64 and Linux x86_64 only")
    return tag


def clean_environment(work: Path) -> dict:
    keep = {key: value for key, value in os.environ.items()
            if not key.startswith(("EREBUS_", "UV_", "PIP_", "VIRTUAL_ENV", "PYTHON"))}
    keep.update({"UV_CACHE_DIR": str(work / "uv-cache"), "UV_HTTP_TIMEOUT": "300", "HOME": str(work / "home")})
    return keep


def install(version: str, work: Path) -> Path:
    uv = shutil.which("uv")
    if uv is None:
        raise ValueError("uv is required")
    environment = clean_environment(work)
    (work / "home").mkdir()
    subprocess.run([uv, "venv", "--python", "3.11", str(work / "env")], env=environment, cwd=work, check=True,
                   capture_output=True)
    subprocess.run([uv, "pip", "install", "--python", str(work / "env/bin/python"), "--no-config", "--index", INDEX,
                    "--default-index", "https://pypi.org/simple", "--index-strategy", "first-index",
                    f"erebus-mcp-server=={version}"], env=environment, cwd=work, check=True, capture_output=True)
    return work / "env/bin"


def verify_installed(bins: Path, version: str, work: Path) -> dict:
    raw = https(f"{RELEASES}/v{version}/release-manifest.json", 4 << 20)
    manifest = json.loads(raw)
    tag = wheel_tag()
    platforms = [entry for entry in manifest["platforms"].values() if entry["platform_qualification"]["wheel_tag"] == tag]
    if manifest.get("version") != version or len(platforms) != 1:
        raise ValueError("the release manifest has no qualified build of this version for this platform")
    expected = {**platforms[0]["binaries"], **platforms[0].get("launchers", {})}
    mismatched = [name for name, record in expected.items()
                  if not (bins / name).is_file() or hashlib.sha256((bins / name).read_bytes()).hexdigest() != record["sha256"]]
    if mismatched:
        raise ValueError(f"installed commands differ from the release: {mismatched}")
    probe = subprocess.run([str(bins / "python"), "-I", "-c",
                            "import erebus, erebus_mcp, importlib.metadata as m; print(erebus.__file__); "
                            "print(erebus_mcp.__file__); print([m.version(p) for p in ('erebus-sdk','erebus-cli','erebus-mcp-server')])"],
                           env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": str(work / "home")}, capture_output=True, text=True, check=True)
    lines = probe.stdout.splitlines()
    if any(not Path(line).resolve().is_relative_to(work / "env") for line in lines[:2]) or lines[2] != str([version] * 3):
        raise ValueError("imports or package versions escaped the fresh environment")
    return {"release_manifest_sha256": hashlib.sha256(raw).hexdigest(),
            "source_commit": manifest.get("source_commit"), "labels": manifest.get("labels"), "wheel_tag": tag,
            "commands_checked": len(expected)}


def prove(bins: Path, work: Path, request: dict) -> tuple[int, dict]:
    path = work / "request.json"
    path.write_text(json.dumps(request))
    path.chmod(0o600)
    started = time.monotonic()
    result = subprocess.run([str(bins / "erebus-local-prove")], input=json.dumps(request), capture_output=True, text=True,
                            cwd=work, env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": str(work / "home")}, timeout=1800)
    try:
        reply = json.loads(result.stdout)
    except ValueError:
        reply = {"status": "error", "error": "unparseable reply"}
    reply.pop("calldata", None)
    reply["elapsed_ms"] = int((time.monotonic() - started) * 1000)
    return result.returncode, reply


def prove_pool(bins: Path, work: Path, tag: str, pin: str, publisher: str, witnesses: Path) -> dict:
    root = work / f"pool-{tag}"
    root.mkdir(mode=0o700)
    manifest = https(f"{RELEASES}/artifacts-268bf1abe34d/manifest-{tag}.json")
    if hashlib.sha256(manifest).hexdigest() != pin:
        raise ValueError(f"{tag}: the hosted manifest is not the authorized one")
    (root / "manifest.json").write_bytes(manifest)
    signature = json.loads(https(f"{RELEASES}/artifacts-268bf1abe34d/manifest-{tag}.signature.json"))
    results = {}
    for kind, fields in TRANSITIONS.items():
        cache = root / f"cache-{kind}"
        cache.mkdir(mode=0o700)
        witness = root / f"{kind}-witness.json"
        shutil.copyfile(witnesses / f"{kind}-input.json", witness)
        witness.chmod(0o600)
        values = json.loads(witness.read_text())
        request = {"manifest_file": str(root / "manifest.json"), "manifest_sha256": pin, "circuit": kind,
                   "manifest_authorization": {"publisher": publisher, "signature": signature},
                   "cache_root": str(cache), "witness_file": str(witness),
                   "expected_public": [values[field] for field in fields], "allow_test_artifacts": True}
        code, reply = prove(bins, root, request)
        if code != 0 or reply.get("status") != "ok" or reply.get("cache_hits") != 0 or not reply.get("downloaded_bytes"):
            raise ValueError(f"{tag} {kind}: installed proving over HTTPS failed: {reply.get('error')}")
        results[kind] = {key: reply.get(key) for key in ("downloaded_bytes", "cache_hits", "installation_ms", "proving_ms", "elapsed_ms")}
    refusal = root / "cache-refusal"
    refusal.mkdir(mode=0o700)
    code, reply = prove(bins, root, {**request, "circuit": "deposit", "cache_root": str(refusal),
                                     "manifest_authorization": {"publisher": "0x" + "11" * 20, "signature": signature}})
    if code == 0 or any(refusal.iterdir()):
        raise ValueError(f"{tag}: a wrong publisher was not refused before installation")
    return {"manifest_sha256": pin, "transport": "https", "publisher": publisher, "transitions": results,
            "verified": True, "wrong_publisher_refused": True}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--version", required=True)
    parser.add_argument("--authorization", type=Path, required=True)
    parser.add_argument("--witnesses", action="append", default=[], help="pool=DIR, one per authorized pool")
    parser.add_argument("--work", type=Path, help="new working directory (default: a fresh one under the system temp)")
    args = parser.parse_args()
    import tempfile
    work = (args.work or Path(tempfile.mkdtemp(prefix="erebus-public-install-"))).resolve()
    work.mkdir(mode=0o700, exist_ok=args.work is None)
    authorization = json.loads(args.authorization.read_text())
    witnesses = dict(item.split("=", 1) for item in args.witnesses)
    pools = authorization.get("hosting_manifests") or {}
    try:
        if authorization.get("package_version") != args.version:
            raise ValueError("the authorization names a different version")
        if set(witnesses) != set(pools):
            raise ValueError(f"give --witnesses for exactly {sorted(pools)}")
        bins = install(args.version, work)
        report = {"status": "verified", "version": args.version, "installed": verify_installed(bins, args.version, work),
                  "pools": {tag: prove_pool(bins, work, tag, pin, authorization["publisher"], Path(witnesses[tag]).resolve())
                            for tag, pin in sorted(pools.items())},
                  "fresh_cache": True, "outside_checkout": not work.is_relative_to(Path(__file__).resolve().parents[1]),
                  "native_runner": os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted",
                  "live_payment": False, "witnesses": "synthetic offline fixtures",
                  "labels": ["test-only", "single-operator", "unaudited"]}
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(json.dumps({"status": "failed", "error": str(error)}))
        return 1
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
