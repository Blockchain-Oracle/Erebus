"""Measure a hash-pinned local prover without publishing witness or proof contents."""

import argparse
import hashlib
import json
import platform
import re
import resource
import stat
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            result.update(chunk)
    return result.hexdigest()


def qualify(binary, expected_hash, request_path, timeout=600, allow_source_build=False,
            environment=None, cwd=None):
    system, machine = platform.system(), platform.machine()
    if (system, machine) not in {("Darwin", "arm64"), ("Linux", "x86_64")}:
        raise ValueError("unsupported qualification platform")
    if not 1 <= timeout <= 3600 or not re.fullmatch(r"[0-9a-f]{64}", expected_hash):
        raise ValueError("invalid qualification configuration")
    if not stat.S_ISREG(binary.lstat().st_mode) or digest(binary) != expected_hash:
        raise ValueError("binary identity mismatch")
    source_build = binary.resolve().is_relative_to(ROOT)
    if source_build and not allow_source_build:
        raise ValueError("source build requires explicit qualification opt-in")
    metadata = request_path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o077 or metadata.st_size > 16384:
        raise ValueError("request must be a bounded owner-only regular file")
    request_bytes = request_path.read_bytes()
    request = json.loads(request_bytes)
    manifest_path = Path(request["manifest_file"])
    if not stat.S_ISREG(manifest_path.lstat().st_mode) or manifest_path.stat().st_size > 16384:
        raise ValueError("invalid manifest")
    manifest_bytes = manifest_path.read_bytes()
    if hashlib.sha256(manifest_bytes).hexdigest() != request["manifest_sha256"]:
        raise ValueError("manifest identity mismatch")
    manifest = json.loads(manifest_bytes)
    if type(manifest.get("test_only")) is not bool:
        raise ValueError("manifest must declare its fixture boundary")
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    start = time.monotonic()
    timed_out = False
    try:
        result = subprocess.run([str(binary.resolve())], input=request_bytes, capture_output=True,
                                timeout=timeout, check=False, env=environment, cwd=cwd)
    except subprocess.TimeoutExpired:
        timed_out = True
        result = None
    elapsed_ms = round((time.monotonic() - start) * 1000)
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    # macOS reports bytes; Linux reports KiB. This is a cumulative child high-water,
    # not a per-stage allocation measurement or a sum of simultaneous process RSS.
    peak = after.ru_maxrss if system == "Darwin" else after.ru_maxrss * 1024
    report = {
        "platform": {"system": system, "machine": machine},
        "binary_sha256": expected_hash,
        "manifest_sha256": request["manifest_sha256"],
        "test_only": manifest["test_only"],
        "source_build": source_build,
        "elapsed_ms": elapsed_ms,
        "child_cpu_ms": round(((after.ru_utime + after.ru_stime) - (before.ru_utime + before.ru_stime)) * 1000),
        "child_peak_rss_bytes": peak,
        "rss_scope": "cumulative_child_high_water",
        "timed_out": timed_out,
        "verified": False,
    }
    if result is None:
        return report
    if len(result.stdout) > 65536:
        return report
    try:
        response = json.loads(result.stdout)
    except (ValueError, UnicodeError):
        return report
    fields = ("downloaded_bytes", "cache_hits", "installation_ms", "proving_ms")
    if not isinstance(response, dict) or response.get("status") != "ok" or result.returncode != 0:
        return report
    if any(type(response.get(name)) is not int or response[name] < 0 for name in fields):
        return report
    report.update({name: response[name] for name in fields})
    report["verified"] = True
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--request", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--allow-source-build", action="store_true")
    args = parser.parse_args()
    try:
        report = qualify(args.binary, args.binary_sha256, args.request, args.timeout, args.allow_source_build)
    except (OSError, ValueError, KeyError, TypeError):
        print(json.dumps({"verified": False, "error": "qualification input rejected"}))
        return 2
    report["rss_scope"] = "prover_child_high_water"
    print(json.dumps(report, sort_keys=True))
    return 0 if report["verified"] else 2


if __name__ == "__main__":
    sys.exit(main())
