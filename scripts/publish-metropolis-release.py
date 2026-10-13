#!/usr/bin/env python3
"""Qualify two downloaded registry-run artifacts and publish them to the Metropolis registry.

    publish-metropolis-release.py --artifacts DIR --authorization FILE --run-id ID [--publish]

DIR holds `metropolis-registry-macos-arm64/` and `metropolis-registry-linux-x86_64/` exactly as
`gh run download <registry run>` writes them. Without --publish nothing leaves this machine.
With it, a new prerelease is created on PoulavBhowmick03/erebus-metropolis (never overwritten),
the wheels and release-manifest.json are uploaded, and the registry's Pages index is dispatched.
The stable Starknet index is never touched.

Publication also requires, for both platforms:
- the run named by --run-id succeeded on the packages' source commit, every job except the
  skipped publish job included, and it was a `publish=false` hosted qualification;
- the owner authorization file names this version, the pinned publisher and both hosting manifests;
- packages from a clean, release-eligible tree that recorded exactly those hosting manifests and
  covered every pool of the deployment record;
- installed qualification reports for install, assets, metered and x402 that verified against this
  `release.json`, ran on a native GitHub-hosted runner, proved every pool over real HTTPS with the
  pinned publisher, and passed every refusal; and a verified HTTPS fetch report per pool;
- a generated separate-agent `workflow.json` (prepublication or published pins, every authorized pool) and a
  funded installed-relayer `relayer.json`, each verified, native, bound to this `release.json`, its source
  commit and the installed binary hashes. Rehearsal-pin reports and in-process relayer runs never qualify.
The binary inventory comes from the registry builder, not a hardcoded count.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "PoulavBhowmick03/erebus-metropolis"
PLATFORMS = {"macos-arm64": ("macosx_11_0_arm64", "macho-arm64"), "linux-x86_64": ("linux_x86_64", "elf-x86_64")}
REHEARSALS = {"assets": "installed_asset_rehearsal", "metered": "installed_metered_rehearsal",
              "x402": "installed_x402_rehearsal"}
LABELS = ["test-only", "single-operator", "unaudited"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def expected_binaries() -> set[str]:
    spec = importlib.util.spec_from_file_location("metropolis_registry_inventory", ROOT / "scripts/build-metropolis-registry.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return {name for names in module.BINARY_GROUPS.values() for name in names}


def native(blob: bytes, kind: str) -> bool:
    if kind == "macho-arm64":
        return blob[:4] == b"\xcf\xfa\xed\xfe" and int.from_bytes(blob[4:8], "little") == 0x0100000C
    return blob[:6] == b"\x7fELF\x02\x01" and int.from_bytes(blob[18:20], "little") == 62


def check_authorization(authorization: dict) -> list[str]:
    problems = []
    if authorization.get("artifact_publication_authorized") is not True:
        problems.append("authorization does not authorize publication")
    if authorization.get("qualification_publish") is not False:
        problems.append("authorization must record a publish=false qualification run")
    if authorization.get("mainnet_authorized") is not False:
        problems.append("this script publishes testnet packages only")
    if authorization.get("repository") != REPOSITORY:
        problems.append("authorization names a different registry repository")
    if not re.fullmatch(r"0x[0-9a-f]{40}", str(authorization.get("publisher", ""))) \
            or authorization["publisher"] == "0x" + "0" * 40:
        problems.append("authorization must pin a lowercase publisher address")
    manifests = authorization.get("hosting_manifests") or {}
    if not manifests or not all(re.fullmatch(r"[0-9a-f]{64}", str(pin)) for pin in manifests.values()):
        problems.append("authorization must pin every hosting manifest")
    for flag, expected in (("test_only", True), ("single_operator", True), ("audited", False)):
        if authorization.get(flag) is not expected:
            problems.append(f"authorization must keep {flag}={str(expected).lower()}")
    return problems


def check_run(run_id: str, commit: str, view=None) -> list[str]:
    """The named run must be the successful qualification of exactly this commit."""
    view = view or (lambda: json.loads(gh("run", "view", run_id, "--repo", "PoulavBhowmick03/Erebus",
                                          "--json", "conclusion,headSha,event,jobs,workflowName").stdout or "{}"))
    run = view()
    problems = []
    if run.get("workflowName") != "Metropolis registry" or run.get("event") != "workflow_dispatch":
        problems.append("qualification must be a dispatched Metropolis registry run")
    if run.get("conclusion") != "success":
        problems.append(f"qualification run concluded {run.get('conclusion')}")
    if run.get("headSha") != commit:
        problems.append("qualification run built a different commit than the packages")
    jobs = {job["name"]: job.get("conclusion") for job in run.get("jobs", [])}
    for name in ("macos-arm64", "linux-x86_64", "Compare platform builds"):
        if jobs.get(name) != "success":
            problems.append(f"job {name} did not succeed")
    if jobs.get("Publish registry") not in (None, "skipped"):
        problems.append("qualification run must not have published")
    return problems


def check_reports(platform: str, root: Path, manifest: dict, release_sha256: str, authorization: dict) -> list[str]:
    problems = []
    pins = set((authorization.get("hosting_manifests") or {}).values())
    publisher = authorization.get("publisher")
    mode = root / "proving-mode"
    if not mode.is_file() or mode.read_text().strip() != "hosted":
        problems.append(f"{platform}: not qualified against hosted artifacts")
    candidate = manifest.get("candidate") or {}
    if candidate.get("release_eligible") is not True:
        problems.append(f"{platform}: packages are not release eligible")
    recorded = {domain.get("manifest_sha256") for domain in manifest.get("proving_domains") or []}
    if recorded != pins:
        problems.append(f"{platform}: packages recorded {sorted(recorded)}, not the authorized hosting manifests")
    if not (manifest.get("deployment_coverage") or {}).get("complete"):
        problems.append(f"{platform}: packages do not cover the deployment")
    reports = {}
    for gate in ("install", *REHEARSALS):
        path = root / "qualification" / f"{gate}.json"
        try:
            report = json.loads(path.read_text())
        except (OSError, ValueError):
            problems.append(f"{platform}: missing {gate} qualification")
            continue
        reports[gate] = report
        claims = report.get("claims") or {}
        if report.get("publication_evidence") is False or report.get("rehearsal") or "rehearsal-pins" in (report.get("labels") or []):
            problems.append(f"{platform}: {gate} is a rehearsal report, which is never publication evidence")
        if report.get("status") != "verified" or report.get("source_imports") is not False:
            problems.append(f"{platform}: {gate} qualification did not verify")
        if report.get("release_json_sha256") != release_sha256:
            problems.append(f"{platform}: {gate} qualified a different release.json")
        if claims.get("native_platform_qualification") is not True:
            problems.append(f"{platform}: {gate} did not run on a native hosted runner from a clean tree")
        if claims.get("live_payment") is not False:
            problems.append(f"{platform}: {gate} claims a live payment")
        if gate in REHEARSALS and report.get(REHEARSALS[gate]) is not True:
            problems.append(f"{platform}: {gate} rehearsal did not run")
    install = reports.get("install") or {}
    if install and install.get("installed_onboarding") is None:
        problems.append(f"{platform}: onboarding did not run")
    coverage = install.get("deployment_coverage") or {}
    if install and not (coverage.get("complete") and coverage.get("pools") and all(p.get("proved") for p in coverage["pools"])):
        problems.append(f"{platform}: installed proving did not cover every deployed pool")
    hosted = install.get("installed_hosted_artifacts") or []
    if install and {report.get("manifest_sha256") for report in hosted} != pins:
        problems.append(f"{platform}: hosted proving did not cover exactly the authorized manifests")
    for report in hosted:
        name = report.get("settlement_contract")
        refusals = report.get("authorization_refusals") or {}
        if not (report.get("proved") is True and report.get("https_exercised") is True and report.get("release_recorded") is True):
            problems.append(f"{platform}: {name} was not proved over real HTTPS from recorded packages")
        if report.get("publisher") != publisher:
            problems.append(f"{platform}: {name} was authorized by a different publisher")
        if not refusals or not all(refusals.values()) or report.get("cache_tamper") not in {"redownloaded", "refused"}:
            problems.append(f"{platform}: {name} refusals incomplete")
    problems += check_feature_gates(platform, root, manifest, release_sha256, authorization)
    fetched = set()
    for path in sorted((root / "qualification").glob("fetch-*.json")):
        try:
            fetch = json.loads(path.read_text())
        except ValueError:
            fetch = {}
        if fetch.get("hosted") is not True:
            problems.append(f"{platform}: {path.name} did not verify")
        fetched.add(fetch.get("manifest_sha256"))
    if fetched != pins:
        problems.append(f"{platform}: HTTPS fetch reports do not cover every authorized manifest")
    return problems


def bound_report(platform: str, root: Path, gate: str, manifest: dict, release_sha256: str) -> tuple[dict, list[str]]:
    """A feature-gate report that must be verified, native, non-rehearsal and bound to these exact packages."""
    path = root / "qualification" / f"{gate}.json"
    try:
        report = json.loads(path.read_text())
    except (OSError, ValueError):
        return {}, [f"{platform}: missing {gate} report"]
    problems = []
    claims = report.get("claims") or {}
    if report.get("status") != "verified" or report.get("source_imports") is not False:
        problems.append(f"{platform}: {gate} report did not verify")
    if report.get("publication_evidence") is False or report.get("rehearsal") or "rehearsal-pins" in (report.get("labels") or []):
        problems.append(f"{platform}: {gate} is a rehearsal report, which is never publication evidence")
    if report.get("release_json_sha256") != release_sha256 or report.get("source_commit") != manifest.get("source_commit"):
        problems.append(f"{platform}: {gate} report is stale or for other packages")
    if claims.get("native_platform_qualification") is not True:
        problems.append(f"{platform}: {gate} did not run on a native hosted runner from a clean tree")
    if claims.get("live_payment") is not False:
        problems.append(f"{platform}: {gate} claims a live payment")
    installed = report.get("installed_binaries_sha256") or {}
    if not installed or any(manifest["binaries"].get(name, {}).get("sha256") != digest for name, digest in installed.items()):
        problems.append(f"{platform}: {gate} report does not bind the installed binaries to these packages")
    return report, problems


def check_feature_gates(platform: str, root: Path, manifest: dict, release_sha256: str, authorization: dict) -> list[str]:
    """The generated separate-agent workflow and the funded installed-relayer gate, both bound to these packages."""
    pools = set(authorization.get("hosting_manifests") or {})
    workflow, problems = bound_report(platform, root, "workflow", manifest, release_sha256)
    if workflow:
        if workflow.get("pins_mode") not in {"prepublication", "published"}:
            problems.append(f"{platform}: workflow operators were not pinned to this authorized version")
        if not {"erebus-negotiate", "erebus-payment", "erebus-wallet"} <= set(workflow.get("installed_binaries_sha256") or {}):
            problems.append(f"{platform}: workflow report does not bind the agent binaries")
        ran = workflow.get("pools") or {}
        if set(ran) != pools:
            problems.append(f"{platform}: workflow did not cover exactly the authorized pools")
        for name, pool in ran.items():
            deal = (pool.get("main") or {}).get("deal") or {}
            if not (deal.get("payment_verified_by_recovery") is True and deal.get("settlement_broadcasts") == 1
                    and (deal.get("delivery") or {}).get("resource_verified") is True
                    and (deal.get("auditor") or {}).get("payment_verified") is True):
                problems.append(f"{platform}: {name} workflow did not verify payment, delivery and audit separately")
    relayer, found = bound_report(platform, root, "relayer", manifest, release_sha256)
    problems += found
    if relayer:
        if "erebus-shielded-relayer" not in (relayer.get("installed_binaries_sha256") or {}):
            problems.append(f"{platform}: relayer report does not bind the installed relayer binary")
        if relayer.get("service") != "installed-binary" or relayer.get("in_process") is not False:
            problems.append(f"{platform}: relayer gate did not drive the installed binary")
        checks = relayer.get("checks") or {}
        for check in RELAYER_CHECKS:
            entry = checks.get(check)
            if not isinstance(entry, dict) or entry.get("passed") is not True or not entry.get("scope") or not entry.get("test"):
                problems.append(f"{platform}: relayer gate missing {check}")
        # Transport-only evidence never stands in for the installed payment CLI or MCP delegating to the relayer.
        delegation = checks.get("installed_payment_delegation") or {}
        if delegation.get("test") in TRANSPORT_ONLY_TESTS:
            problems.append(f"{platform}: relayer delegation is covered only at the transport level")
        mcp = checks.get("installed_mcp_delegation") or {}
        if mcp.get("test") in TRANSPORT_ONLY_TESTS or mcp.get("test") == delegation.get("test"):
            problems.append(f"{platform}: MCP sponsorship lacks its own installed tool test")
        if relayer.get("gate_complete") is not True or relayer.get("coverage_gaps"):
            problems.append(f"{platform}: relayer gate reports coverage gaps {relayer.get('coverage_gaps')}")
    return problems


RELAYER_CHECKS = ("submit", "status", "restart_recovery", "sponsor_sender", "dropped_broadcast_resumed",
                  "duplicate_one_broadcast", "no_direct_fallback", "installed_payment_delegation", "installed_mcp_delegation")
# Tests that drive only the library RelayEndpoint transport, not the installed erebus-payment CLI or MCP tool.
TRANSPORT_ONLY_TESTS = {"buyer_transport_delegates_to_the_installed_binary_and_never_signs_directly"}


def qualify(artifacts: Path, authorization: dict | None = None) -> tuple[dict, dict[str, Path], list[str]]:
    manifests, assets, problems = {}, {}, []
    binaries = expected_binaries()
    authorization = authorization or {}
    for platform, (tag, kind) in PLATFORMS.items():
        root = artifacts / f"metropolis-registry-{platform}"
        release_bytes = (root / "release.json").read_bytes()
        manifest = json.loads(release_bytes)
        manifests[platform] = manifest
        if manifest["dirty_source"] or manifest["published"] or manifest.get("test_only_artifacts_included"):
            problems.append(f"{platform}: dirty, already published, or includes test-only artifacts")
        if manifest["platform_qualification"]["wheel_tag"] != tag:
            problems.append(f"{platform}: wheel tag {manifest['platform_qualification']['wheel_tag']} is not {tag}")
        if manifest.get("licenses", {}).get("project") != "Apache-2.0":
            problems.append(f"{platform}: manifest records no project license")
        if authorization and manifest.get("version") != authorization.get("package_version"):
            problems.append(f"{platform}: version {manifest.get('version')} is not the authorized version")
        for name, meta in manifest["wheels"].items():
            path = root / "wheels" / name
            data = path.read_bytes()
            if sha256(data) != meta["sha256"]:
                problems.append(f"{platform} {name}: hash does not match the manifest")
            with zipfile.ZipFile(path) as archive:
                names = archive.namelist()
                licenses = {n.rsplit("/", 1)[-1] for n in names if ".dist-info/licenses/" in n}
                record = [n for n in names if n.endswith(".dist-info/WHEEL")]
                tags = [line for line in archive.read(record[0]).decode().splitlines() if line.startswith("Tag:")]
                if name.endswith("-py3-none-any.whl"):
                    if "LICENSE" not in licenses:
                        problems.append(f"{name}: no LICENSE")
                else:
                    if tags != [f"Tag: py3-none-{tag}"] or not {"LICENSE", "THIRD_PARTY_NOTICES"} <= licenses:
                        problems.append(f"{name}: tag {tags} or licenses {sorted(licenses)}")
                    for binary, binary_meta in {**manifest["binaries"], **manifest.get("launchers", {})}.items():
                        hits = [n for n in names if n.endswith(f"/scripts/{binary}")]
                        blob = archive.read(hits[0]) if len(hits) == 1 else b""
                        if sha256(blob) != binary_meta["sha256"]:
                            problems.append(f"{platform} {binary}: missing or hash mismatch")
                        elif binary in manifest["binaries"] and not native(blob, kind):
                            problems.append(f"{platform} {binary}: not a {kind} executable")
            if name in assets and assets[name].read_bytes() != data:
                problems.append(f"{name}: differs between platforms")
            assets.setdefault(name, path)
        if set(manifest["binaries"]) != binaries:
            problems.append(f"{platform}: native binaries {sorted(manifest['binaries'])} are not the build inventory "
                            f"of {len(binaries)}")
        if authorization:
            problems.extend(check_reports(platform, root, manifest, sha256(release_bytes), authorization))
    commits = {m["source_commit"] for m in manifests.values()}
    versions = {m["version"] for m in manifests.values()}
    if len(commits) != 1 or len(versions) != 1:
        problems.append(f"platforms disagree: commits {commits}, versions {versions}")
    trees = {(m.get("candidate") or {}).get("source_tree_sha256") for m in manifests.values()}
    if len(trees) != 1:
        problems.append(f"platforms packaged different input trees: {sorted(map(str, trees))}")
    version = versions.pop()
    combined = {"version": version, "tag": "v" + version, "channel": "metropolis-testnet",
                "source_commit": commits.pop(), "build_profile": manifests["macos-arm64"]["build_profile"],
                "labels": LABELS, "platforms": {m["platform"]: m for m in manifests.values()}}
    return combined, assets, problems


def gh(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["gh", *args], capture_output=True, text=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--authorization", type=Path, required=True,
                        help="owner release authorization (packaging/metropolis/testnet-release.json)")
    parser.add_argument("--run-id", required=True, help="the publish=false hosted qualification run the artifacts came from")
    parser.add_argument("--publish", action="store_true")
    args = parser.parse_args()
    authorization_bytes = args.authorization.read_bytes()
    authorization = json.loads(authorization_bytes)
    problems = check_authorization(authorization)
    combined, assets, more = qualify(args.artifacts.resolve(), authorization)
    problems += more + check_run(args.run_id, combined["source_commit"])
    combined["authorization"] = {"sha256": sha256(authorization_bytes), **authorization}
    combined["qualification_run"] = f"https://github.com/PoulavBhowmick03/Erebus/actions/runs/{args.run_id}"
    summary = {"version": combined["version"], "source_commit": combined["source_commit"],
               "qualification_run": combined["qualification_run"], "assets": sorted(assets), "problems": problems}
    if problems:
        print(json.dumps({"status": "not_qualified", **summary}, indent=2))
        raise SystemExit(1)
    tag = combined["tag"]
    if gh("release", "view", tag, "--repo", REPOSITORY).returncode == 0:
        print(json.dumps({"status": "refused", "reason": f"{tag} already exists; releases are never overwritten"}))
        raise SystemExit(1)
    if not args.publish:
        print(json.dumps({"status": "qualified", "published": False, **summary}, indent=2))
        return
    with tempfile.TemporaryDirectory() as staging:
        manifest = Path(staging) / "release-manifest.json"
        manifest.write_text(json.dumps(combined, indent=2) + "\n")
        created = gh("release", "create", tag, "--repo", REPOSITORY, "--prerelease", "--title", f"Metropolis {combined['version']}",
                     "--notes", f"Built from {combined['source_commit']}, qualified by {combined['qualification_run']}. "
                     "Experimental Metropolis testnet packages: test-only setup, single-operator, unaudited; "
                     "not independently accepted. See release-manifest.json for wheel, native, launcher, license, "
                     "proving-manifest and qualification hashes.")
        if created.returncode:
            sys.exit(f"release creation failed: {created.stderr.strip()}")
        uploaded = gh("release", "upload", tag, "--repo", REPOSITORY, *[str(path) for path in assets.values()], str(manifest))
        if uploaded.returncode:
            sys.exit(f"upload failed after creating {tag}; fix the assets of that release, do not recreate it: {uploaded.stderr.strip()}")
    dispatched = gh("api", f"repos/{REPOSITORY}/dispatches", "-f", "event_type=metropolis-registry", "-f", f"client_payload[tag]={tag}")
    print(json.dumps({"status": "published", "tag": tag, "pages_dispatched": dispatched.returncode == 0, **summary}, indent=2))


if __name__ == "__main__":
    main()
