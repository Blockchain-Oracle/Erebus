#!/usr/bin/env python3
"""One-step operator setup for a published Metropolis release. Never broadcasts; never reads key contents.

    operator_init.py pins    --version V --directory NEW
    operator_init.py init    --role buyer|seller --pool wmon|usdc --directory NEW --bin DIR --pins DIR
                             --deployment FILE --endpoint HOST:PORT --rpc-url URL --peer-rpc-url URL
                             --signer-address 0x.. [--transaction-key-file PATH] [--maximum-price N]
    operator_init.py connect --directory DIR --peer-share DIR --resource NAME --amount N
                             [--payload-file FILE --port N] (seller)   [--minimum-price N]
    operator_init.py doctor  --directory DIR [--offline]
    operator_init.py wallet-policy --directory DIR --maximum-amount N [--allow METHOD ...]
                             [--withdrawal-recipient ADDR ...] [--replacement-fees MAX PRIORITY]
    operator_init.py auditor --pool wmon|usdc --directory NEW --bin DIR --deployment FILE
                             --rpc-url URL --peer-rpc-url URL

`pins` downloads, over HTTPS only, the package release's `release-manifest.json`, the owner authorization and
deployment record at that release's source commit, and every authorized pool's hosting manifest and signature,
and checks the manifests against the authorization. Its output directory is the `--pins` input of `init`.

`init` checks the installed commands against the release manifest's hashes for this platform and pins the
publisher-signed hosting manifest of the chosen pool (from a `--pins` directory holding
`release-manifest.json`, `manifest-<pool>.json`, its `.signature.json` and `testnet-release.json`). It then
asks the installed `erebus-negotiate` to create the suite-2 operator keys and identity binding, and writes
owner-only `wallet.json` (and `payment.json` for a buyer) with the pool's five runtime pins. Key files are
created by Rust; this script only passes paths. The gas key is the operator's own: give its address and,
optionally, its key file path (omit the file to sign externally). A `share/` directory receives the public
descriptor and binding for the counterparty, plus the seller's offer after `connect`.

`connect` runs once the counterparty's `share/` directory has arrived. It asks `erebus-negotiate` to prepare
the terms template and writes `negotiate.json`, the seller's `access.json`, and `mcp.json` (environment for
`erebus-mcp-server` with `EREBUS_BACKEND=metropolis`).

`doctor` is read-only and prints no secrets. It reports whether the installed commands still match the hashes
recorded at `init`, whether every configured key file, config and state directory is owner-only (key files
are never opened), whether the pinned manifest is unchanged, how much of it is cached, whether the wallet
opens (note count only), and, unless `--offline`, whether both RPC endpoints pass the installed network check
on chain 10143 and the gas signer's native balance on each.

`auditor` creates an auditor key with the installed `erebus-shielded-disclosure` and renders the disclosure
deployment config (see `disclosure_config.py`).

Amounts are base units of the pool's asset. Everything is test-only, single-operator and unaudited, like the
deployment it names.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMESPACE_CHAIN = 10143
COMMANDS = ("erebus-negotiate", "erebus-payment", "erebus-wallet", "erebus-access", "erebus-access-service",
            "erebus-shielded-disclosure", "erebus-artifacts", "erebus-local-prove", "erebus-mcp-server")
ADDRESS = re.compile(r"0x[0-9a-f]{40}", re.ASCII)
UNSIGNED = re.compile(r"[0-9]{1,78}", re.ASCII)
GAS = {"gas_limit": 3000000, "max_fee_per_gas": "300000000000", "max_priority_fee_per_gas": "2000000000"}


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def private_json(path: Path, data: dict) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        stream.write(json.dumps(data, indent=2) + "\n")


def wheel_tag() -> str:
    system, machine = platform.system(), platform.machine()
    if (system, machine) == ("Darwin", "arm64"):
        return "macosx_11_0_arm64"
    if (system, machine) == ("Linux", "x86_64"):
        return "linux_x86_64"
    raise ValueError("published releases support macOS arm64 and Linux x86_64 only")


def release_pins(pins: Path, pool: str) -> dict:
    """The authorized hosting manifest, signature and publisher for one pool, checked against each other."""
    authorization = json.loads((pins / "testnet-release.json").read_text())
    manifest = pins / f"manifest-{pool}.json"
    signature = pins / f"manifest-{pool}.signature.json"
    expected = (authorization.get("hosting_manifests") or {}).get(pool)
    if expected is None or sha256(manifest) != expected:
        raise ValueError("the pool's hosting manifest does not match the release authorization")
    publisher = authorization.get("publisher", "")
    if not ADDRESS.fullmatch(publisher):
        raise ValueError("the release authorization does not pin a publisher")
    release = json.loads((pins / "release-manifest.json").read_text())
    if release.get("version") != authorization.get("package_version"):
        raise ValueError("the release manifest is not the authorized package version")
    return {"manifest": manifest, "manifest_sha256": expected, "signature": json.loads(signature.read_text()),
            "publisher": publisher, "release": release, "test_only": json.loads(manifest.read_text()).get("test_only")}


def verify_installed(bins: Path, release: dict) -> dict:
    """Every installed command must be the release's byte-for-byte; returns the checked hashes."""
    tag = wheel_tag()
    platforms = [entry for entry in release["platforms"].values() if entry["platform_qualification"]["wheel_tag"] == tag]
    if len(platforms) != 1:
        raise ValueError("the release has no qualified build for this platform")
    expected = {**platforms[0]["binaries"], **platforms[0].get("launchers", {})}
    checked = {}
    for name, record in expected.items():
        path = bins / name
        if path.is_symlink() or not path.is_file() or sha256(path) != record["sha256"]:
            raise ValueError(f"installed {name} is not the published binary")
        checked[name] = record["sha256"]
    return checked


def command(bins: Path, name: str, request: dict, cwd: Path) -> dict:
    result = subprocess.run([str(bins / name)], input=json.dumps(request), capture_output=True, text=True,
                            cwd=cwd, timeout=120, check=False,
                            env={"PATH": f"{bins}{os.pathsep}/usr/bin:/bin", "HOME": os.environ.get("HOME", str(cwd))})
    try:
        reply = json.loads(result.stdout)
    except ValueError:
        reply = {}
    if result.returncode != 0 or reply.get("status") != "ok":
        raise ValueError(f"{name} {request.get('method')} refused: {reply.get('error', 'no reply')}")
    return reply


def pool_entry(deployment: Path, name: str) -> tuple[dict, dict]:
    record = json.loads(deployment.read_text())
    matches = [entry for entry in record.get("pools", []) if entry.get("name") == name]
    if len(matches) != 1 or record.get("chain_id") != NAMESPACE_CHAIN:
        raise ValueError("the deployment record must list this pool on chain 10143")
    return record, matches[0]


def require_unsigned(value: str, label: str) -> str:
    if not UNSIGNED.fullmatch(value):
        raise ValueError(f"{label} must be a base-unit integer")
    return value


REGISTRY = "https://github.com/PoulavBhowmick03/erebus-metropolis/releases/download"
SOURCE = "https://raw.githubusercontent.com/PoulavBhowmick03/Erebus"


def https(url: str, opener=None) -> bytes:
    import urllib.request
    if not url.startswith("https://"):
        raise ValueError("pins are downloaded over https only")
    with (opener or urllib.request.urlopen)(url, timeout=120) as response:
        data = response.read((8 << 20) + 1)
    if len(data) > 8 << 20:
        raise ValueError(f"unexpectedly large download: {url}")
    return data


def pins(args, opener=None) -> dict:
    directory = args.directory.resolve()
    if directory.exists():
        raise ValueError("use a new pins directory")
    release_bytes = https(f"{REGISTRY}/v{args.version}/release-manifest.json", opener)
    release = json.loads(release_bytes)
    commit = str(release.get("source_commit", ""))
    if release.get("version") != args.version or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("the release manifest does not name this version and a source commit")
    authorization_bytes = https(f"{SOURCE}/{commit}/packaging/metropolis/testnet-release.json", opener)
    authorization = json.loads(authorization_bytes)
    deployment_bytes = https(f"{SOURCE}/{commit}/contracts/evm/deployments/monad-testnet-shielded-v2.json", opener)
    if authorization.get("package_version") != args.version:
        raise ValueError("the authorization at the release commit names a different version")
    files = {"release-manifest.json": release_bytes, "testnet-release.json": authorization_bytes,
             "monad-testnet-shielded-v2.json": deployment_bytes}
    for pool, pin in sorted((authorization.get("hosting_manifests") or {}).items()):
        manifest = https(f"{REGISTRY}/{authorization['artifact_tag']}/manifest-{pool}.json", opener)
        if hashlib.sha256(manifest).hexdigest() != pin:
            raise ValueError(f"{pool}: hosted manifest differs from the authorization")
        files[f"manifest-{pool}.json"] = manifest
        files[f"manifest-{pool}.signature.json"] = https(f"{REGISTRY}/{authorization['artifact_tag']}/manifest-{pool}.signature.json", opener)
    directory.mkdir(mode=0o700)
    for name, data in files.items():
        (directory / name).write_bytes(data)
    return {"status": "pinned", "directory": str(directory), "source_commit": commit, "publisher": authorization.get("publisher"),
            "files": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())},
            "note": "publisher and digests come from the release's own commit; confirm them through a channel you trust"}


def init(args) -> dict:
    directory = args.directory.resolve()
    if directory.exists():
        raise ValueError("use a new operator directory")
    signer = args.signer_address.lower()
    if not ADDRESS.fullmatch(signer) or signer == "0x" + "0" * 40:
        raise ValueError("--signer-address must be the operator's own gas address")
    if args.transaction_key_file is not None and not args.transaction_key_file.is_file():
        raise ValueError("--transaction-key-file must name an existing owner-only key file")
    if args.role == "buyer":
        require_unsigned(args.maximum_price or "", "--maximum-price")
    disclosure = load("disclosure_config")
    disclosure.endpoint(args.rpc_url)
    disclosure.endpoint(args.peer_rpc_url)
    if args.rpc_url == args.peer_rpc_url:
        raise ValueError("the primary and peer RPC endpoints must differ")
    bins = args.bin.resolve()
    record, pool = pool_entry(args.deployment, args.pool)
    pins = release_pins(args.pins.resolve(), args.pool)
    checked = verify_installed(bins, pins["release"])
    manifest = json.loads(pins["manifest"].read_text())
    if str(manifest.get("settlement_contract")).lower() != pool["pool"].lower() or manifest.get("chain_id") != NAMESPACE_CHAIN:
        raise ValueError("the hosting manifest names a different pool")
    staging = load("stage_artifacts")
    identity = staging.deployment_identity(args.deployment, args.pool)
    for circuit in manifest["circuits"]:
        if circuit["identity"]["verifier_runtime_keccak256"] != identity["runtime_pins"][circuit["kind"]]:
            raise ValueError("the hosting manifest's verifiers differ from the deployment pins")

    directory.mkdir(mode=0o700, parents=False)
    (directory / "share").mkdir(mode=0o700)
    state = directory / "state"
    state.mkdir(mode=0o700)
    (state / "artifacts").mkdir(mode=0o700)
    artifacts = directory / "release"
    artifacts.mkdir(mode=0o700)
    shutil.copyfile(pins["manifest"], artifacts / "manifest.json")
    namespace = f"eip155:{NAMESPACE_CHAIN}"
    asset = pool["asset"].lower()
    operator = command(bins, "erebus-negotiate", {
        "method": "prepare_operator", "suite_id": 2, "directory": str(directory / "keys"), "role": args.role,
        "endpoint": args.endpoint, "namespace": namespace, "assets": [f"{namespace}/erc20:{asset}"],
        "descriptor_lifetime_seconds": args.descriptor_lifetime_seconds}, directory)
    binding = directory / "keys" / "identity.binding"
    command(bins, "erebus-negotiate", {
        "method": "prepare_binding", "output": str(binding), "descriptor_file": operator["descriptor_file"],
        "discovery_key_file": operator["discovery_key_file"], "agreement_key_file": operator["agreement_key_file"],
        "settlement_contract": pool["pool"].lower(), "verifier_version": pool["verifier_version"],
        "expires_at": int(time.time()) + args.binding_lifetime_seconds}, directory)
    for source in (Path(operator["descriptor_file"]), binding):
        shutil.copyfile(source, directory / "share" / source.name)
        (directory / "share" / source.name).chmod(0o600)
    authorization = {"publisher": pins["publisher"], "signature": pins["signature"]}
    transaction_key = str(args.transaction_key_file.resolve()) if args.transaction_key_file else None
    wallet = {
        "version": 1, "state_root": str(state), "namespace": namespace, "pool": pool["pool"].lower(),
        "verifier_version": pool["verifier_version"], "asset": asset,
        "wrapper": asset if args.pool == "wmon" else None, "rpc_url": args.rpc_url, "peer_rpc_url": args.peer_rpc_url,
        "first_block": pool["deployment_block"], "first_hash": pool["deployment_hash"].lower(),
        "wallet_file": str(state / "wallet" / "notes.enc"), "wallet_key_file": operator["wallet_key_file"],
        "agreement_key_file": operator["wallet_agreement_key_file"], "signer_address": signer,
        "signer_journal_root": str(state / "signer-journal"), "transaction_key_file": transaction_key,
        "artifact_manifest_file": str(artifacts / "manifest.json"), "artifact_manifest_sha256": pins["manifest_sha256"],
        "artifact_manifest_authorization": authorization, "artifact_cache_root": str(state / "artifacts"),
        "runtime_pins": {name: value for name, value in identity["runtime_pins"].items()},
        "allow_test_artifacts": pins["test_only"] is True, "allow_loopback_artifact_http": False,
        **GAS, "timeout_seconds": 30, "max_scan_blocks": 1000, "artifact_timeout_seconds": 300,
    }
    private_json(directory / "wallet.json", wallet)
    written = ["wallet.json"]
    if args.role == "buyer":
        public = Path(operator["agreement_key_file"]).with_name("agreement.public")
        if not public.is_file():
            raise ValueError("erebus-negotiate did not write the public agreement key")
        pins_hex = {field: "0x" + identity["runtime_pins"][pin] for field, pin in load("disclosure_config").PINS.items()}
        private_json(directory / "payment.json", {
            "version": 1, "mode": "shielded", "state_root": str(state), "namespace": namespace,
            "pool": pool["pool"].lower(), "verifier_version": pool["verifier_version"], **pins_hex,
            "first_block": pool["deployment_block"], "first_hash": pool["deployment_hash"].lower(),
            "rpc_url": args.rpc_url, "peer_rpc_url": args.peer_rpc_url, "buyer_key_hex": public.read_bytes().hex(),
            "asset": f"{namespace}/erc20:{asset}", "signer_address": signer,
            "signer_journal_root": wallet["signer_journal_root"], "transaction_key_file": transaction_key,
            "wallet_file": wallet["wallet_file"], "wallet_key_file": wallet["wallet_key_file"],
            "manifest_file": wallet["artifact_manifest_file"], "manifest_sha256": pins["manifest_sha256"],
            "manifest_authorization": authorization, "artifact_cache": wallet["artifact_cache_root"],
            "allow_test_artifacts": wallet["allow_test_artifacts"], "allow_loopback_http": False,
            "maximum_price": args.maximum_price, **GAS, "timeout_seconds": 30, "max_scan_blocks": 1000})
        written.append("payment.json")
    summary = {"version": 1, "role": args.role, "pool": args.pool, "settlement_contract": pool["pool"].lower(),
               "asset": asset, "chain_id": NAMESPACE_CHAIN, "endpoint": args.endpoint, "signer_address": signer,
               "bin": str(bins), "release_version": pins["release"]["version"],
               "release_source_commit": pins["release"]["source_commit"], "binaries_sha256": checked,
               "hosting_manifest_sha256": pins["manifest_sha256"], "publisher": pins["publisher"],
               "deployment_sha256": sha256(args.deployment), "operator": {k: v for k, v in operator.items() if k.endswith("_file")},
               "labels": ["test-only", "single-operator", "unaudited"], "transactions_sent": 0}
    private_json(directory / "operator.json", summary)
    return {"status": "initialized", "directory": str(directory), "written": written + ["operator.json", "share/"],
            "next": "fund the signer address with testnet MON, send share/ to the counterparty, then run connect"}


def connect(args) -> dict:
    directory = args.directory.resolve()
    operator = json.loads((directory / "operator.json").read_text())
    bins = Path(operator["bin"])
    verify_installed(bins, release_pins_from(operator))
    peer = args.peer_share.resolve()
    descriptors = sorted(peer.glob("*.descriptor.json"))
    if len(descriptors) != 1 or not (peer / "identity.binding").is_file():
        raise ValueError("the peer share must hold exactly one descriptor and identity.binding")
    role, other = operator["role"], ("seller" if operator["role"] == "buyer" else "buyer")
    require_unsigned(args.amount, "--amount")
    namespace = f"eip155:{NAMESPACE_CHAIN}"
    offer_file = peer / "offer.json"
    if role == "seller":
        if args.payload_file is None or args.port is None:
            raise ValueError("a seller connects with --payload-file and --port")
        digest = sha256(args.payload_file)
        deadline = int(time.time()) + args.delivery_seconds
        offer = {"resource": args.resource, "unit": args.unit, "quantity": args.quantity, "price": args.amount,
                 "fulfillment_method": "http-access-v1", "fulfillment_digest": digest, "delivery_deadline": deadline}
    else:
        if not offer_file.is_file():
            raise ValueError("a buyer connects after the seller's share includes offer.json")
        offer = json.loads(offer_file.read_text())
        if offer.get("resource") != args.resource:
            raise ValueError("the seller's offer names a different resource")
        digest, deadline = offer["fulfillment_digest"], offer["delivery_deadline"]
    files = operator["operator"]
    terms = directory / "keys" / "service.terms"
    command(bins, "erebus-negotiate", {
        "method": "prepare_terms", "suite_id": 2, "output": str(terms), "role": role,
        "local_descriptor_file": files["descriptor_file"], "peer_descriptor_file": str(descriptors[0]),
        "local_binding_file": str(directory / "keys" / "identity.binding"), "peer_binding_file": str(peer / "identity.binding"),
        "settlement_contract": operator["settlement_contract"], "verifier_version": 2,
        "asset": f"{namespace}/erc20:{operator['asset']}", "amount": args.amount, "resource": args.resource,
        "unit": offer["unit"], "quantity": offer["quantity"], "fulfillment_method": offer["fulfillment_method"],
        "fulfillment_digest": digest, "delivery_deadline": deadline}, directory)
    maximum = json.loads((directory / "payment.json").read_text())["maximum_price"] if role == "buyer" else args.amount
    negotiate = {
        "version": 1, "role": role, "state_root": str(directory / "state"),
        "transport_key_file": files["transport_key_file"], "agreement_key_file": files["agreement_key_file"],
        "discovery_key_file": files["discovery_key_file"], "local_descriptor_file": files["descriptor_file"],
        "peer_descriptor_file": str(descriptors[0]), "terms_template_file": str(terms), "endpoint": operator["endpoint"],
        "maximum_price": maximum, "minimum_price": require_unsigned(args.minimum_price or args.amount, "--minimum-price"),
        "max_deal_lifetime_seconds": args.delivery_seconds, "timeout_seconds": 30}
    written = ["negotiate.json", "mcp.json"]
    environment = {"EREBUS_BACKEND": "metropolis", "EREBUS_NEGOTIATION_CONFIG": str(directory / "negotiate.json"),
                   "EREBUS_NEGOTIATION_CLI": str(bins / "erebus-negotiate")}
    if role == "seller":
        wallet = json.loads((directory / "wallet.json").read_text())
        evidence = directory / "access-evidence"
        evidence.mkdir(mode=0o700)
        negotiate.update({"seller_spend_secret_file": files["seller_spend_secret_file"],
                          "seller_wallet_file": wallet["wallet_file"], "seller_wallet_key_file": wallet["wallet_key_file"],
                          "access_evidence_root": str(evidence)})
        public = Path(files["agreement_key_file"]).with_name("agreement.public")
        pins = wallet["runtime_pins"]
        payload = directory / "payload"
        shutil.copyfile(args.payload_file, payload)
        payload.chmod(0o600)
        private_json(directory / "access.json", {
            "service_id": list(os.urandom(32)), "seller_key": list(public.read_bytes()), "suite_id": 2,
            "resource": args.resource, "payload_file": str(payload), "evidence_root": str(evidence),
            "state_root": str(directory / "access-state"), "port": args.port,
            "backend": {"mode": "shielded", "chain_id": NAMESPACE_CHAIN, "pool": operator["settlement_contract"],
                        "rpc_url": wallet["rpc_url"], "peer_rpc_url": wallet["peer_rpc_url"],
                        "first_block": wallet["first_block"], "first_hash": list(bytes.fromhex(wallet["first_hash"][2:])),
                        "runtime_keccak256": pins["pool"], "poseidon_keccak256": pins["poseidon"],
                        "deposit_verifier_keccak256": pins["deposit"], "transfer_verifier_keccak256": pins["transfer"],
                        "withdraw_verifier_keccak256": pins["withdraw"], "max_concurrent_queries": 4}})
        written.append("access.json")
        offer_out = directory / "share" / "offer.json"
        private_json(offer_out, offer)
    else:
        environment.update({"EREBUS_PAYMENT_CONFIG": str(directory / "payment.json"),
                            "EREBUS_PAYMENT_CLI": str(bins / "erebus-payment")})
    private_json(directory / "negotiate.json", negotiate)
    private_json(directory / "mcp.json", {"mcpServers": {f"erebus-{role}": {"command": str(bins / "erebus-mcp-server"),
                                                                          "env": environment}}})
    return {"status": "connected", "role": role, "peer": other, "written": written,
            "next": ("start erebus-access-service with access.json, then the MCP server" if role == "seller"
                     else "add mcp.json to the agent, fund the wallet (wrap/approve/deposit with erebus-wallet), then negotiate")}


def release_pins_from(operator: dict) -> dict:
    """The binary hashes checked at init time; connect refuses commands that changed since."""
    return {"platforms": {"local": {"platform_qualification": {"wheel_tag": wheel_tag()},
                                    "binaries": {name: {"sha256": digest} for name, digest in operator["binaries_sha256"].items()}}}}


def rpc_balance(url: str, address: str) -> int:
    import urllib.request
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [address, "finalized"]}).encode()
    with urllib.request.urlopen(urllib.request.Request(url, body, {"content-type": "application/json"}), timeout=30) as response:
        return int(json.loads(response.read(1 << 16))["result"], 16)


def network_check(bins: Path, url: str, cwd: Path) -> dict:
    """The installed read-only network check; its reply carries `passed`, not `status`."""
    result = subprocess.run([str(bins / "erebus-network-check")], capture_output=True, text=True, timeout=120, cwd=cwd,
                            input=json.dumps({"chain_id": NAMESPACE_CHAIN, "rpc_url": url, "timeout_ms": 15000}),
                            env={"PATH": f"{bins}{os.pathsep}/usr/bin:/bin", "HOME": str(cwd)})
    return json.loads(result.stdout)


def doctor(args) -> dict:
    directory = args.directory.resolve()
    operator = json.loads((directory / "operator.json").read_text())
    bins = Path(operator["bin"])
    checks, problems = {}, []

    def owner_only(path: Path, directory_expected: bool = False) -> bool:
        info = path.lstat()
        kind_ok = stat.S_ISDIR(info.st_mode) if directory_expected else stat.S_ISREG(info.st_mode)
        return kind_ok and not stat.S_IMODE(info.st_mode) & 0o077

    try:
        verify_installed(bins, release_pins_from(operator))
        checks["installed_commands"] = "match the release"
    except ValueError as error:
        problems.append(str(error))
    configs = [name for name in ("wallet.json", "payment.json", "negotiate.json", "access.json", "mcp.json", "operator.json")
               if (directory / name).exists()]
    keys = [Path(path) for name, path in operator["operator"].items() if name.endswith("_file") and "descriptor" not in name]
    wallet = json.loads((directory / "wallet.json").read_text())
    if wallet.get("transaction_key_file"):
        keys.append(Path(wallet["transaction_key_file"]))
    loose = [str(path.relative_to(directory)) if path.is_relative_to(directory) else path.name
             for path in [directory / name for name in configs] + keys if not path.exists() or not owner_only(path)]
    loose += [name for name in ("state", "keys", "share") if not owner_only(directory / name, True)]
    if loose:
        problems.append(f"not owner-only or missing: {loose}")
    checks["owner_only"] = {"configs": len(configs), "key_files": len(keys), "problems": loose}
    manifest = Path(wallet["artifact_manifest_file"])
    if sha256(manifest) != wallet["artifact_manifest_sha256"] or wallet["artifact_manifest_sha256"] != operator["hosting_manifest_sha256"]:
        problems.append("the pinned artifact manifest changed")
    circuits = json.loads(manifest.read_text())["circuits"]
    wanted = [circuit[field]["sha256"] for circuit in circuits for field in ("wasm", "r1cs", "zkey")]
    cache = Path(wallet["artifact_cache_root"])
    checks["artifact_cache"] = {"cached": sum((cache / f"{digest}.artifact").exists() for digest in wanted), "of": len(wanted),
                                "note": "missing artifacts download over HTTPS on first proof"}
    try:
        notes = command(bins, "erebus-wallet", {"method": "notes", "config_file": str(directory / "wallet.json")}, directory)
        checks["wallet"] = {"opens": True, "notes": len(notes["result"]["notes"]), "operations": len(notes["result"]["operations"])}
    except ValueError as error:
        problems.append(f"wallet does not open: {error}")
    if not args.offline:
        endpoints = {}
        for label in ("rpc_url", "peer_rpc_url"):
            url = wallet[label]
            try:
                check = network_check(bins, url, directory)
                endpoints[label] = {"passed": check.get("passed"), "finalized": (check.get("finalized") or {}).get("number"),
                                    "signer_balance_wei": rpc_balance(url, wallet["signer_address"])}
                if check.get("passed") is not True:
                    problems.append(f"{label} failed the network check")
            except (OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
                endpoints[label] = {"passed": False}
                problems.append(f"{label} unreachable: {type(error).__name__}")
        checks["endpoints"] = endpoints
        if all(entry.get("signer_balance_wei") == 0 for entry in endpoints.values()):
            problems.append("the gas signer has no testnet MON yet")
    return {"status": "healthy" if not problems else "attention", "role": operator["role"], "pool": operator["pool"],
            "release_version": operator["release_version"], "signer_address": operator["signer_address"],
            "checks": checks, "problems": problems, "secrets_printed": False}


WALLET_READS = ("notes", "sync")
WALLET_WRITES = ("approve", "wrap", "unwrap", "deposit", "withdraw", "resume", "replace")
# Optional read actions: never enabled by default, each gated on an installed capability.
WALLET_OPTIONAL_READS = {"status": "paired_account_diagnostics"}


def replace_private_json(path: Path, data: dict) -> None:
    """Rewrite an owner-only file atomically; the new file is created 0600 before any bytes are written."""
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    private_json(temporary, data)
    os.replace(temporary, path)


def wallet_policy(args) -> dict:
    """Write an explicit wallet action policy and add the wallet to this operator's MCP settings.

    Reads (notes, sync) are always allowed; every write method must be named with --allow. Funding methods
    are enabled only when the installed erebus-wallet advertises durable operation references."""
    directory = args.directory.resolve()
    operator = json.loads((directory / "operator.json").read_text())
    bins = Path(operator["bin"])
    verify_installed(bins, release_pins_from(operator))
    maximum = require_unsigned(args.maximum_amount, "--maximum-amount")
    if int(maximum) == 0:
        raise ValueError("--maximum-amount must be positive")
    allowed = set(args.allow)
    unknown = allowed - set(WALLET_WRITES) - set(WALLET_OPTIONAL_READS)
    if unknown:
        raise ValueError(f"unsupported wallet methods: {sorted(unknown)}")
    if {"wrap", "unwrap"} & allowed and operator["pool"] != "wmon":
        raise ValueError("wrap and unwrap apply only to the wrapped-native (wmon) pool")
    recipients = sorted({value.lower() for value in args.withdrawal_recipient})
    if any(not ADDRESS.fullmatch(value) or value == "0x" + "0" * 40 for value in recipients):
        raise ValueError("withdrawal recipients must be canonical nonzero addresses")
    if "withdraw" in allowed and not recipients:
        raise ValueError("allowing withdraw requires at least one --withdrawal-recipient")
    if recipients and "withdraw" not in allowed:
        raise ValueError("--withdrawal-recipient applies only when withdraw is allowed")
    fees = None
    if "replace" in allowed:
        if args.replacement_fees is None:
            raise ValueError("allowing replace requires --replacement-fees MAX_FEE PRIORITY_FEE")
        fees = [require_unsigned(value, "--replacement-fees") for value in args.replacement_fees]
        if int(fees[1]) > int(fees[0]):
            raise ValueError("the replacement priority fee exceeds its cap")
    elif args.replacement_fees is not None:
        raise ValueError("--replacement-fees applies only when replace is allowed")
    # Same reading as the SDK's MetropolisWallet.version(): the envelope's result advertises protocol, methods
    # and durable-reference capabilities.
    capabilities = command(bins, "erebus-wallet", {"method": "version"}, directory).get("result") or {}
    if capabilities.get("protocol") != 1 or not set(WALLET_READS) | allowed <= set(capabilities.get("methods") or []):
        raise ValueError("the installed erebus-wallet does not advertise the requested methods")
    if "deposit" in allowed and capabilities.get("deposit_operation_references") is not True:
        raise ValueError("the installed erebus-wallet lacks durable deposit references; deposit stays disabled")
    if {"approve", "wrap", "unwrap"} & allowed and capabilities.get("funding_operation_references") is not True:
        raise ValueError("the installed erebus-wallet lacks durable funding references; approve/wrap/unwrap stay disabled")
    for method, capability in WALLET_OPTIONAL_READS.items():
        if method in allowed and capabilities.get(capability) is not True:
            raise ValueError(f"the installed erebus-wallet lacks {capability}; {method} stays disabled")
    policy = {"version": 1, "maximum_amount": maximum, "allowed_methods": sorted(set(WALLET_READS) | allowed),
              "withdrawal_recipients": recipients}
    if fees is not None:
        policy["replacement_fees"] = fees
    target = directory / "wallet-policy.json"
    (replace_private_json if target.exists() else private_json)(target, policy)
    written = ["wallet-policy.json"]
    mcp = directory / "mcp.json"
    if mcp.exists():
        settings = json.loads(mcp.read_text())
        entry = next(iter(settings["mcpServers"].values()))
        entry["env"].update({"EREBUS_WALLET_CONFIG": str(directory / "wallet.json"), "EREBUS_WALLET_POLICY": str(target),
                             "EREBUS_WALLET_CLI": str(bins / "erebus-wallet")})
        replace_private_json(mcp, settings)
        written.append("mcp.json")
    return {"status": "wallet-policy-written", "directory": str(directory), "written": written,
            "allowed_methods": policy["allowed_methods"], "maximum_amount": maximum,
            "funding_enabled": sorted(allowed & {"approve", "wrap", "unwrap", "deposit"}),
            "next": "restart the MCP server; agents can call only the methods above, each capped at maximum_amount"
            if mcp.exists() else "run connect to generate mcp.json, then re-run wallet-policy"}


def auditor(args) -> dict:
    directory = args.directory.resolve()
    if directory.exists():
        raise ValueError("use a new auditor directory")
    disclosure = load("disclosure_config")
    directory.mkdir(mode=0o700)
    bins = args.bin.resolve()
    command(bins, "erebus-shielded-disclosure", {"method": "keygen", "key_file": str(directory / "auditor.key")}, directory)
    (directory / "cache").mkdir(mode=0o700)
    private_json(directory / "disclosure.json", disclosure.render(args.deployment, args.pool, args.rpc_url,
                                                                  args.peer_rpc_url, str(directory / "cache")))
    return {"status": "auditor-ready", "directory": str(directory), "written": ["auditor.key (Rust)", "disclosure.json"]}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    pinning = commands.add_parser("pins")
    pinning.add_argument("--version", required=True)
    pinning.add_argument("--directory", type=Path, required=True)
    first = commands.add_parser("init")
    first.add_argument("--role", choices=("buyer", "seller"), required=True)
    first.add_argument("--pool", choices=("wmon", "usdc"), required=True)
    first.add_argument("--directory", type=Path, required=True)
    first.add_argument("--bin", type=Path, required=True, help="installed release bin directory")
    first.add_argument("--pins", type=Path, required=True, help="release pins directory")
    first.add_argument("--deployment", type=Path, required=True)
    first.add_argument("--endpoint", required=True, help="host:port this operator's negotiation listens on")
    first.add_argument("--rpc-url", required=True)
    first.add_argument("--peer-rpc-url", required=True)
    first.add_argument("--signer-address", required=True)
    first.add_argument("--transaction-key-file", type=Path)
    first.add_argument("--maximum-price", help="buyer: highest price in base units")
    first.add_argument("--descriptor-lifetime-seconds", type=int, default=86400)
    first.add_argument("--binding-lifetime-seconds", type=int, default=21600)
    second = commands.add_parser("connect")
    second.add_argument("--directory", type=Path, required=True)
    second.add_argument("--peer-share", type=Path, required=True)
    second.add_argument("--resource", required=True)
    second.add_argument("--amount", required=True, help="this side's opening price in base units")
    second.add_argument("--minimum-price")
    second.add_argument("--unit", default="snapshot")
    second.add_argument("--quantity", default="1")
    second.add_argument("--payload-file", type=Path)
    second.add_argument("--port", type=int)
    second.add_argument("--delivery-seconds", type=int, default=21600)
    checkup = commands.add_parser("doctor")
    checkup.add_argument("--directory", type=Path, required=True)
    checkup.add_argument("--offline", action="store_true", help="skip RPC endpoint and balance checks")
    policy = commands.add_parser("wallet-policy", help="explicit agent wallet permissions (default: notes and sync only)")
    policy.add_argument("--directory", type=Path, required=True)
    policy.add_argument("--maximum-amount", required=True, help="per-action cap in base units")
    policy.add_argument("--allow", action="append", default=[], choices=(*WALLET_WRITES, *WALLET_OPTIONAL_READS),
                        help="repeatable: a write method, or the optional status read, agents may call; none by default")
    policy.add_argument("--withdrawal-recipient", action="append", default=[], help="repeatable; required with --allow withdraw")
    policy.add_argument("--replacement-fees", nargs=2, metavar=("MAX_FEE", "PRIORITY_FEE"), help="required with --allow replace")
    third = commands.add_parser("auditor")
    third.add_argument("--pool", choices=("wmon", "usdc"), required=True)
    third.add_argument("--directory", type=Path, required=True)
    third.add_argument("--bin", type=Path, required=True)
    third.add_argument("--deployment", type=Path, required=True)
    third.add_argument("--rpc-url", required=True)
    third.add_argument("--peer-rpc-url", required=True)
    args = parser.parse_args(argv)
    try:
        result = {"pins": pins, "init": init, "connect": connect, "doctor": doctor, "auditor": auditor,
                  "wallet-policy": wallet_policy}[args.command](args)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({"status": "error", "error": str(error) if isinstance(error, ValueError) else type(error).__name__}))
        return 2
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
