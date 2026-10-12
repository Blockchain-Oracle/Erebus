#!/usr/bin/env python3
"""Render or check a shielded disclosure deployment config for one deployed pool. Offline; never signs.

    disclosure_config.py render --deployment FILE --pool NAME --rpc-url URL --peer-rpc-url URL --cache-root DIR
    disclosure_config.py check  --config FILE --release release.json [--deployment FILE --pool NAME]

`render` copies the pool's identity from a deployment record (the `pools` list Codex records): namespace,
pool address, verifier version, deployment block and hash, and the five runtime pins (pool, Poseidon,
deposit, transfer and withdrawal verifiers) as `0x`-prefixed lowercase digests. The two RPC endpoints and
the cache root are operator inputs and have no defaults.

`check` refuses a config unless the package `release.json` recorded the strict disclosure schema it was
built with and the config has exactly those fields, every pin is a non-zero canonical digest, the
endpoints are distinct, and (with a deployment record) every identity field matches that pool. Packages
built before the schema was recorded are refused rather than guessed compatible; nothing here relaxes the
installed binary's own validation.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.parse
from pathlib import Path

BINARY = "erebus-shielded-disclosure"
PINS = {"runtime_keccak256": "pool", "poseidon_keccak256": "poseidon", "deposit_verifier_keccak256": "deposit",
        "transfer_verifier_keccak256": "transfer", "withdraw_verifier_keccak256": "withdraw"}
DIGEST = re.compile(r"0x[0-9a-f]{64}", re.ASCII)
ADDRESS = re.compile(r"0x[0-9a-f]{40}", re.ASCII)


def pool_entry(deployment: Path, name: str) -> tuple[dict, dict]:
    record = json.loads(deployment.read_text())
    matches = [entry for entry in record.get("pools", []) if entry.get("name") == name]
    if len(matches) != 1:
        raise ValueError("the deployment record must list exactly one pool with that name")
    return record, matches[0]


def canonical(value: str) -> str:
    digits = str(value).lower().removeprefix("0x")
    if not re.fullmatch(r"[0-9a-f]{64}", digits) or digits == "0" * 64:
        raise ValueError("runtime pins and block hashes must be non-zero 32-byte hex")
    return "0x" + digits


def endpoint(url: str) -> str:
    parts = urllib.parse.urlsplit(url)
    if parts.scheme not in {"https", "http"} or not parts.hostname or parts.username or parts.password:
        raise ValueError("RPC endpoints must be http(s) URLs without credentials")
    return url


def render(deployment: Path, name: str, rpc_url: str, peer_rpc_url: str, cache_root: str) -> dict:
    _, pool = pool_entry(deployment, name)
    if endpoint(rpc_url) == endpoint(peer_rpc_url):
        raise ValueError("the primary and peer RPC endpoints must differ")
    if not cache_root:
        raise ValueError("a cache root is required")
    pins = pool.get("runtime_pins") or {}
    return {
        "namespace": pool["namespace"],
        "settlement_contract": str(pool["pool"]).lower(),
        "verifier_version": pool["verifier_version"],
        "rpc_url": rpc_url,
        "peer_rpc_url": peer_rpc_url,
        "first_block": pool["deployment_block"],
        "first_hash": canonical(pool["deployment_hash"]),
        "cache_root": cache_root,
        **{field: canonical(pins[pin]) for field, pin in PINS.items()},
    }


def check(config: dict, release: dict, deployment: Path | None = None, name: str | None = None) -> dict:
    problems: list[str] = []
    schema = (release.get("deployment_schemas") or {}).get(BINARY)
    if schema is None:
        problems.append("the packages did not record the disclosure deployment schema; rebuild them from the current source")
    elif set(config) != set(schema["fields"]):
        missing, extra = sorted(set(schema["fields"]) - set(config)), sorted(set(config) - set(schema["fields"]))
        problems.append(f"config fields differ from the packaged schema (missing {missing}, unknown {extra})")
    for field in PINS:
        if not DIGEST.fullmatch(str(config.get(field, ""))) or config.get(field) == "0x" + "0" * 64:
            problems.append(f"{field} must be a non-zero 0x-prefixed lowercase digest")
    if not DIGEST.fullmatch(str(config.get("first_hash", ""))):
        problems.append("first_hash must be a 0x-prefixed lowercase digest")
    if not ADDRESS.fullmatch(str(config.get("settlement_contract", ""))):
        problems.append("settlement_contract must be a lowercase address")
    if config.get("rpc_url") == config.get("peer_rpc_url"):
        problems.append("the primary and peer RPC endpoints must differ")
    if (deployment is None) != (name is None):
        raise ValueError("--deployment and --pool go together")
    if deployment is not None:
        _, pool = pool_entry(deployment, name)
        expected = render(deployment, name, "https://primary.check/", "https://peer.check/", "-")
        for field in ("namespace", "settlement_contract", "verifier_version", "first_block", "first_hash", *PINS):
            if config.get(field) != expected[field]:
                problems.append(f"{field} does not match the deployment record's {name} pool")
    return {"binary": BINARY, "schema_fields": (schema or {}).get("fields"), "pool": name,
            "problems": problems, "accepted": not problems}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    rendering = commands.add_parser("render")
    rendering.add_argument("--deployment", type=Path, required=True)
    rendering.add_argument("--pool", required=True)
    rendering.add_argument("--rpc-url", required=True)
    rendering.add_argument("--peer-rpc-url", required=True)
    rendering.add_argument("--cache-root", required=True)
    checking = commands.add_parser("check")
    checking.add_argument("--config", type=Path, required=True)
    checking.add_argument("--release", type=Path, required=True)
    checking.add_argument("--deployment", type=Path)
    checking.add_argument("--pool")
    args = parser.parse_args()
    try:
        if args.command == "render":
            print(json.dumps(render(args.deployment, args.pool, args.rpc_url, args.peer_rpc_url, args.cache_root), indent=2))
            return 0
        result = check(json.loads(args.config.read_text()), json.loads(args.release.read_text()), args.deployment, args.pool)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({"status": "error", "error": str(error) if isinstance(error, ValueError) else type(error).__name__}))
        return 2
    print(json.dumps({"status": "ok" if result["accepted"] else "refused", **result}, indent=2))
    return 0 if result["accepted"] else 1


if __name__ == "__main__":
    sys.exit(main())
