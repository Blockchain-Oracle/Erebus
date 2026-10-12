#!/usr/bin/env python3
"""Read-only demo sequence for recording the published Metropolis release. Needs no keys; sends nothing.

    metropolis-demo-readonly.py --bin DIR [--rpc-url URL] [--json]

Each section is labelled CURRENT (run now against the installed release or the live chain) or HISTORICAL
(a finalized past transaction read back now). It prints:
  1. CURRENT     installed command versions and their published hashes;
  2. CURRENT     the live Monad testnet read-only network check;
  3. CURRENT     the deployed WMON/USDC pool code identities read at the finalized block;
  4. HISTORICAL  the finalized private WMON payment receipt (2026-10-11), read back now.
Never show key files, grants, note openings, witnesses or operator configuration on screen.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import urllib.request
from pathlib import Path

PAYMENT = "0xe01666d8c4e16efe8195da8047674c71a325ee20ff250f02f4fdf570a04934fa"
POOLS = {"wmon": ("0x8868f35140f3602c80e2c76132740b6f0c0b409d", "4965730fb58c3d2685e29f9857074e5b81d4fa0e21926c6e76800e0fa280f679"),
         "usdc": ("0x863f540471c0ad33c68d1218f31e8c9fe9cd50fb", "1ca881783d272534444d30971b21c1dc0e95235997b089215b2963ffa73b63c5")}
LIMITS = ("Monad testnet only; test-only single-operator setup; unaudited; funding and withdrawals are visible; "
          "the outer gas sender is public; the anonymity set is small; no outside acceptance yet.")


def rpc(url: str, method: str, params: list) -> object:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, body, {"content-type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        reply = json.loads(response.read(8 << 20))
    if "error" in reply:
        raise ValueError(f"{method}: {reply['error']}")
    return reply["result"]


def keccak(data: bytes, bins: Path) -> str | None:
    """Runtime identity via Foundry's cast when present; otherwise the caller reports the byte length only."""
    try:
        result = subprocess.run(["cast", "keccak", "0x" + data.hex()], capture_output=True, text=True, timeout=30)
    except FileNotFoundError:
        return None
    return result.stdout.strip().removeprefix("0x") if result.returncode == 0 else None


def call(bins: Path, name: str, request: dict) -> dict:
    result = subprocess.run([str(bins / name)], input=json.dumps(request), capture_output=True, text=True, timeout=120,
                            env={"PATH": f"{bins}:/usr/bin:/bin", "HOME": "/tmp"})
    return json.loads(result.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin", type=Path, required=True)
    parser.add_argument("--rpc-url", default="https://testnet-rpc.monad.xyz")
    parser.add_argument("--json", action="store_true", help="one JSON document instead of labelled sections")
    args = parser.parse_args()
    bins = args.bin.resolve()
    sections = []
    versions = {name: call(bins, name, {"method": "version"}) for name in ("erebus-negotiate", "erebus-payment")}
    sections.append(("CURRENT", "installed commands", {
        name: {key: reply.get(key) for key in ("service", "protocol_version", "modes", "automatic_rebroadcast", "settlement_submission")
               if key in reply} | {"sha256": hashlib.sha256((bins / name).read_bytes()).hexdigest()} for name, reply in versions.items()}))
    check = call(bins, "erebus-network-check", {"chain_id": 10143, "rpc_url": args.rpc_url, "timeout_ms": 15000})
    sections.append(("CURRENT", "live network check (read-only)", {
        "passed": check.get("passed"), "chain_id": check.get("chain_id"),
        "finalized": (check.get("finalized") or {}).get("number"), "head": (check.get("head") or {}).get("number")}))
    finalized = rpc(args.rpc_url, "eth_getBlockByNumber", ["finalized", False])
    identities = {}
    for name, (address, pin) in POOLS.items():
        code = bytes.fromhex(str(rpc(args.rpc_url, "eth_getCode", [address, finalized["number"]]))[2:])
        digest = keccak(code, bins)
        identities[name] = {"pool": address, "runtime_bytes": len(code),
                            "runtime_keccak256": digest, "matches_deployment_pin": None if digest is None else digest == pin}
    sections.append(("CURRENT", f"deployed pool code at finalized block {int(finalized['number'], 16)}", identities))
    receipt = rpc(args.rpc_url, "eth_getTransactionReceipt", [PAYMENT])
    sections.append(("HISTORICAL", "finalized private WMON payment (2026-10-11), read back now", {
        "transaction": PAYMENT, "status": receipt["status"], "block": int(receipt["blockNumber"], 16), "to": receipt["to"],
        "gas_sender_public": receipt["from"], "log_emitters": sorted({log["address"] for log in receipt["logs"]}),
        "note": "pool note/deal events only, no ERC-20 Transfer; amount and buyer/seller keys are not in the receipt, "
                "but the outer gas sender is public"}))
    if args.json:
        print(json.dumps({"sections": [{"label": label, "title": title, "data": data} for label, title, data in sections],
                          "limits": LIMITS}, indent=2))
    else:
        for label, title, data in sections:
            print(f"\n[{label}] {title}\n{json.dumps(data, indent=2)}")
        print(f"\n[LIMITS] {LIMITS}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
