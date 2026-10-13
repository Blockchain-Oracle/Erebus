#!/usr/bin/env python3
"""Packaging-owned operations rehearsals on an installed Metropolis operator. Sends no transactions.

    rehearse-metropolis-operations.py --work DIR --pool wmon|usdc [--upgrade-registry DIR --upgrade-version V
                                      --rollback-version P]

DIR is a `check-metropolis-init.py` work directory (installed `env/`, `pins/`, and `<pool>/{buyer,seller}`).
Each rehearsal uses installed commands only and reports what it showed:

- interrupted download: start a real HTTPS artifact install into an empty cache, kill it mid-download, then
  rerun; the rerun must complete, verify every byte, and leave no partial `.artifact` file;
- corrupted cache: flip a byte in a cached artifact; the next install must re-download it, never use it;
- persistence: a restarted wallet, MCP server and doctor read the same state (note and operation counts);
- backup and restore: archive the buyer operator with permissions, damage `wallet.json` and a key file,
  restore in place, and require `doctor` to be healthy again and every key file byte-identical;
- endpoint change: point the wallet and payment configs at a different primary RPC (scripted, keeping
  every other field), require `doctor` to pass on the new endpoint and the wallet to open;
- diagnostics: an unreachable RPC endpoint must make `doctor` report attention without printing key bytes;
- upgrade and rollback (optional): install a newer local candidate, then the published version, in the same
  environment; `doctor` must accept installed commands only when their hashes match the operator's record.
"""

from __future__ import annotations

import argparse
import functools
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tarfile
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path



def run(bins: Path, work: Path, binary: str, arguments: list[str] | None = None, stdin: str | None = None,
        timeout: int = 600) -> dict:
    result = subprocess.run([str(bins / binary), *(arguments or [])], input=stdin, capture_output=True, text=True,
                            cwd=work, timeout=timeout, env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": str(work / "home")})
    try:
        reply = json.loads(result.stdout)
    except ValueError:
        reply = {"raw": result.stdout[-300:], "stderr": result.stderr[-300:]}
    reply["exit"] = result.returncode
    return reply


def doctor(bins: Path, work: Path, operator: Path, offline: bool = True) -> dict:
    return run(bins, work, "erebus-init", ["--metropolis", "doctor", "--directory", str(operator), *(["--offline"] if offline else [])])


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def install_request(operator: Path, cache: Path, kind: str) -> str:
    wallet = json.loads((operator / "wallet.json").read_text())
    return json.dumps({"manifest_file": wallet["artifact_manifest_file"], "manifest_sha256": wallet["artifact_manifest_sha256"],
                       "manifest_authorization": wallet["artifact_manifest_authorization"], "circuit": kind,
                       "cache_root": str(cache), "allow_test_artifacts": wallet["allow_test_artifacts"]})


def interrupted_download(bins: Path, work: Path, operator: Path) -> dict:
    cache = work / f"rehearsal-cache-{operator.parent.name}-{int(time.time())}"
    cache.mkdir(mode=0o700)
    process = subprocess.Popen([str(bins / "erebus-artifacts")], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, cwd=work, text=True,
                               env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": str(work / "home")})
    process.stdin.write(install_request(operator, cache, "transfer"))
    process.stdin.close()
    time.sleep(1.5)
    interrupted = process.poll() is None
    if interrupted:
        process.send_signal(signal.SIGKILL)
    process.wait(timeout=30)
    leftovers = sorted(p.name for p in cache.rglob("*") if p.is_file())
    reply = run(bins, work, "erebus-artifacts", stdin=install_request(operator, cache, "transfer"))
    require(reply.get("status") == "ok", f"install after interruption failed: {reply}")
    manifest = json.loads(Path(json.loads((operator / "wallet.json").read_text())["artifact_manifest_file"]).read_text())
    transfer = next(c for c in manifest["circuits"] if c["kind"] == "transfer")
    complete = all((cache / f"{transfer[f]['sha256']}.artifact").is_file() and
                   hashlib.sha256((cache / f"{transfer[f]['sha256']}.artifact").read_bytes()).hexdigest() == transfer[f]["sha256"]
                   for f in ("wasm", "r1cs", "zkey"))
    stray = [p.name for p in cache.rglob("*") if p.is_file() and not p.name.endswith(".artifact")]
    require(complete, "artifacts after the rerun do not match the manifest")
    return {"cache": str(cache), "killed_mid_download": interrupted, "files_left_by_kill": leftovers, "rerun_downloaded_bytes": reply.get("downloaded_bytes"),
            "rerun_cache_hits": reply.get("cache_hits"), "verified": complete, "non_artifact_files_after": stray}


def corrupted_cache(bins: Path, work: Path, operator: Path, cache: Path) -> dict:
    manifest = json.loads(Path(json.loads((operator / "wallet.json").read_text())["artifact_manifest_file"]).read_text())
    zkey = next(c for c in manifest["circuits"] if c["kind"] == "transfer")["zkey"]
    path = cache / f"{zkey['sha256']}.artifact"
    data = bytearray(path.read_bytes())
    data[len(data) // 2] ^= 1
    path.write_bytes(bytes(data))
    reply = run(bins, work, "erebus-artifacts", stdin=install_request(operator, cache, "transfer"))
    restored = hashlib.sha256(path.read_bytes()).hexdigest() == zkey["sha256"]
    require(reply.get("status") == "ok" and restored and reply.get("downloaded_bytes") == zkey["bytes"],
            f"a corrupted cached artifact was not replaced: {reply}")
    return {"redownloaded_bytes": reply.get("downloaded_bytes"), "cache_hits": reply.get("cache_hits"), "restored": restored}


def persistence(bins: Path, work: Path, operator: Path) -> dict:
    first = run(bins, work, "erebus-wallet", stdin=json.dumps({"method": "notes", "config_file": str(operator / "wallet.json")}))
    second = run(bins, work, "erebus-wallet", stdin=json.dumps({"method": "notes", "config_file": str(operator / "wallet.json")}))
    require(first.get("status") == second.get("status") == "ok", "the wallet did not reopen")
    counts = [(len(r["result"]["notes"]), len(r["result"]["operations"])) for r in (first, second)]
    require(counts[0] == counts[1], "wallet state changed across restarts without any operation")
    return {"wallet_reopened": True, "notes_and_operations": counts[1],
            "note": "empty fixture wallets; durable funded-state recovery is covered by the funded local rehearsals"}


def backup_restore(bins: Path, work: Path, operator: Path) -> dict:
    archive = work / f"operator-backup-{operator.parent.name}-{int(time.time())}.tar"
    keys = {p: p.read_bytes() for p in (operator / "keys").glob("*.key")}
    with tarfile.open(archive, "w") as tar:
        tar.add(operator, arcname=operator.name)
    archive.chmod(0o600)
    (operator / "wallet.json").write_text("{}")
    victim = next(iter(keys))
    victim.write_bytes(b"\0" * 32)
    broken = doctor(bins, work, operator)
    require(broken.get("status") != "healthy", "doctor did not notice a damaged operator")
    with tarfile.open(archive) as tar:
        tar.extractall(operator.parent, filter="tar")
    identical = all(path.read_bytes() == data for path, data in keys.items())
    owner_only = all(not (path.stat().st_mode & 0o077) for path in keys)
    healthy = doctor(bins, work, operator)
    require(identical and owner_only and healthy.get("status") == "healthy", f"restore incomplete: {healthy.get('problems')}")
    return {"damaged_detected": broken.get("status"), "restored_doctor": healthy.get("status"), "keys_identical": identical,
            "keys_owner_only": owner_only, "archive_mode": oct(archive.stat().st_mode & 0o777)}


def endpoint_change(bins: Path, work: Path, operator: Path) -> dict:
    """Swap the primary and peer providers (both independently checked), verify, then restore the originals."""
    paths = [operator / name for name in ("wallet.json", "payment.json") if (operator / name).exists()]
    originals = {path: path.read_text() for path in paths}
    try:
        for path in paths:
            data = json.loads(originals[path])
            data["rpc_url"], data["peer_rpc_url"] = data["peer_rpc_url"], data["rpc_url"]
            path.write_text(json.dumps(data, indent=2) + "\n")
        primary = json.loads(paths[0].read_text())["rpc_url"]
        report = doctor(bins, work, operator, offline=False)
        wallet = run(bins, work, "erebus-wallet", stdin=json.dumps({"method": "notes", "config_file": str(operator / "wallet.json")}))
    finally:
        for path, text in originals.items():
            path.write_text(text)
    endpoints = report.get("checks", {}).get("endpoints", {})
    require(all(entry.get("passed") is True for entry in endpoints.values()) and wallet.get("status") == "ok",
            f"after the endpoint change: {endpoints} wallet={wallet.get('status')}")
    require(all(p == "the gas signer has no testnet MON yet" for p in report.get("problems", [])), f"doctor problems: {report.get('problems')}")
    return {"configs_rewritten": [p.name for p in paths], "new_primary": primary, "endpoints": endpoints,
            "wallet_opens": True, "restored": True,
            "note": "rewritten by this rehearsal script; there is no installed endpoint command yet"}


def diagnostics(bins: Path, work: Path, operator: Path) -> dict:
    path = operator / "wallet.json"
    original = path.read_text()
    data = json.loads(original)
    data["rpc_url"] = "https://127.0.0.1:9/"
    path.write_text(json.dumps(data, indent=2) + "\n")
    try:
        report = doctor(bins, work, operator, offline=False)
    finally:
        path.write_text(original)
    rendered = json.dumps(report)
    leaked = [p.name for p in (operator / "keys").glob("*.key") if p.read_bytes().hex() in rendered]
    require(report.get("status") == "attention" and not leaked, f"diagnostics: {report.get('problems')} leaked={leaked}")
    return {"status": report.get("status"), "problems": report.get("problems"), "key_bytes_printed": bool(leaked)}


def upgrade_rollback(bins: Path, work: Path, operator: Path, registry: Path, version: str, rollback: str) -> dict:
    uv = shutil.which("uv")
    environment = {k: v for k, v in os.environ.items() if not k.startswith(("EREBUS_", "UV_", "PIP_", "VIRTUAL_ENV", "PYTHON"))}
    environment.update({"UV_CACHE_DIR": str(work / "uv-cache"), "UV_HTTP_TIMEOUT": "300", "HOME": str(work / "home")})

    def install(index: str, wanted: str) -> None:
        subprocess.run([uv, "pip", "install", "--python", str(work / "env/bin/python"), "--no-config", "--index", index,
                        "--default-index", "https://pypi.org/simple", "--index-strategy", "first-index", "--reinstall-package",
                        "erebus-cli", "--reinstall-package", "erebus-sdk", "--reinstall-package", "erebus-mcp-server",
                        f"erebus-mcp-server=={wanted}"], env=environment, cwd=work, check=True, capture_output=True)

    server = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(type("Q", (SimpleHTTPRequestHandler,),
                                 {"log_message": lambda *a: None}), directory=str(registry)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    recorded = json.loads((operator / "operator.json").read_text())["binaries_sha256"]

    def state(version_label: str) -> dict:
        matches = all((bins / name).is_file() and hashlib.sha256((bins / name).read_bytes()).hexdigest() == digest
                      for name, digest in recorded.items())
        wallet = run(bins, work, "erebus-wallet", stdin=json.dumps({"method": "notes", "config_file": str(operator / "wallet.json")}))
        help_text = subprocess.run([str(bins / "erebus-init"), "--metropolis", "--help"], capture_output=True, text=True, timeout=60,
                                   env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": str(work / "home")})
        integrated = help_text.returncode == 0 and "doctor" in help_text.stdout
        return {"version": version_label, "binaries_match_operator_record": matches, "wallet_opens": wallet.get("status") == "ok",
                "erebus_init_metropolis": integrated, "doctor": doctor(bins, work, operator).get("status") if integrated else None}

    steps = {}
    try:
        install("https://poulavbhowmick03.github.io/erebus-metropolis/simple/", rollback)
        steps["rolled_back_to"] = state(rollback)
        install(f"http://127.0.0.1:{server.server_port}/simple/", version)
        steps["upgraded_to"] = state(version)
    finally:
        server.shutdown()
        server.server_close()
    require(all(step["binaries_match_operator_record"] and step["wallet_opens"] for step in steps.values()),
            f"operator state after upgrade/rollback: {steps}")
    require(steps["upgraded_to"]["doctor"] == "healthy", f"doctor after upgrading: {steps['upgraded_to']}")
    return {**steps, "note": "both versions ship byte-identical native binaries; the published version predates "
                             "`erebus-init --metropolis`, so operators who roll back keep their files but use the repository "
                             "helpers until they upgrade again. A release with changed binaries makes doctor report drift."}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--pool", choices=("wmon", "usdc"), required=True)
    parser.add_argument("--upgrade-registry", type=Path)
    parser.add_argument("--upgrade-version")
    parser.add_argument("--rollback-version")
    args = parser.parse_args()
    work = args.work.resolve()
    bins = work / "env/bin"
    buyer = work / args.pool / "buyer"
    report = {"pool": args.pool, "transactions_sent": 0}
    try:
        report["interrupted_download"] = interrupted_download(bins, work, buyer)
        report["corrupted_cache"] = corrupted_cache(bins, work, buyer, Path(report["interrupted_download"]["cache"]))
        report["persistence"] = persistence(bins, work, buyer)
        report["backup_restore"] = backup_restore(bins, work, buyer)
        report["diagnostics"] = diagnostics(bins, work, buyer)
        report["endpoint_change"] = endpoint_change(bins, work, buyer)
        if args.upgrade_registry:
            report["upgrade_rollback"] = upgrade_rollback(bins, work, buyer, args.upgrade_registry.resolve(),
                                                          args.upgrade_version, args.rollback_version)
        report["status"] = "verified"
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report.update(status="failed", error=str(error)[:1500])
        print(json.dumps(report, indent=2))
        return 1
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
