"""Native generated-agent qualification must use public inputs and exact authorization."""

from pathlib import Path
import ast
import asyncio
import importlib.util
import json
from types import SimpleNamespace

import pytest

ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize("demo_only", [False, True])
def test_demo_mode_preserves_full_default_and_cannot_claim_native_qualification(tmp_path, monkeypatch, demo_only):
    spec = importlib.util.spec_from_file_location("demo_workflow", ROOT / "scripts/rehearse-metropolis-workflow.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    monkeypatch.setattr(module, "CHECKOUT", tmp_path)
    monkeypatch.setenv("GITHUB_ACTIONS", "true")
    monkeypatch.setenv("RUNNER_ENVIRONMENT", "github-hosted")
    (tmp_path / "release.json").write_text(json.dumps({"version": "test", "binaries": {}, "dirty_source": False}))

    class Installed:
        def __init__(self, *args):
            pass

        def init(self, *args, **kwargs):
            if "pins-wrong" in " ".join(args):
                return {"status": "error"}
            return {"rehearsal": True, "publication_evidence": False, "files": {}}

    class Chain:
        def __init__(self, *args):
            pass

        def deploy(self, *args):
            return tmp_path / "deployment.json", {}

        def fixture_keys(self):
            return []

        def sends(self):
            return 0

        def close(self):
            pass

    class Pool:
        def __init__(self, *args, **kwargs):
            self.report = {}

        async def main_pair(self, extended):
            return {"extended": extended}

        async def deadline_pair(self):
            return {"checked": True}

    monkeypatch.setattr(module, "Installed", Installed)
    monkeypatch.setattr(module, "Chain", Chain)
    monkeypatch.setattr(module, "Pool", Pool)
    args = SimpleNamespace(work=tmp_path, registry=tmp_path, pools=["wmon"], authorized_pins=tmp_path,
                           prepublication_authorization=None, deployment_journal=tmp_path,
                           demo_main_only=demo_only, hold_on_failure=False)
    report = asyncio.run(module.workflow(args))
    assert report["pools"]["wmon"]["main"]["extended"] is (not demo_only)
    assert ("deadline" in report["pools"]["wmon"]) is (not demo_only)
    assert report["claims"]["native_platform_qualification"] is (not demo_only)
    if demo_only:
        assert report["gate_complete"] is False and report["publication_evidence"] is False
        assert report["coverage_gaps"] == ["timeout_and_reservation", "backup_restore", "endpoint_change", "deadline"]


def mode_checker():
    source = ast.parse((ROOT / "scripts/rehearse-metropolis-workflow.py").read_text())
    functions = [node for node in source.body if isinstance(node, ast.FunctionDef)
                 and node.name in {"require", "check_operator_mode"}]
    namespace = {"Failure": RuntimeError}
    exec(compile(ast.Module(body=functions, type_ignores=[]), "workflow-mode", "exec"), namespace)
    return namespace["check_operator_mode"]


@pytest.mark.parametrize("prepublication", [False, True])
def test_operator_mode_accepts_only_matching_pins(prepublication):
    check = mode_checker()
    replies = {name: {} if prepublication else {"rehearsal": True}
               for name in ("seller_init", "buyer_init", "auditor")}
    summary = ({"prepublication": True, "labels": ["test-only"]} if prepublication else
               {"labels": ["rehearsal-pins"], "rehearsal": {"publication_evidence": False}})
    check(replies, [summary, summary], prepublication)
    with pytest.raises(RuntimeError):
        check(replies, [summary], not prepublication)
    with pytest.raises(RuntimeError):
        check(replies, [{}], prepublication)


def test_prepublication_refuses_rehearsal_auditor_and_mixed_summary():
    check = mode_checker()
    with pytest.raises(RuntimeError):
        check({"auditor": {"rehearsal": True}}, [{"prepublication": True}], True)
    with pytest.raises(RuntimeError):
        check({}, [{"prepublication": True, "labels": ["rehearsal-pins"]}], True)


def workflow_step():
    text = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    return text.split("      - name: Native generated two-agent workflow from public replay inputs\n", 1)[1].split(
        "      - uses: actions/upload-artifact@v4", 1
    )[0]


def test_generated_gate_is_hosted_only_and_does_not_use_an_operator_journal():
    step = workflow_step()
    assert "if: ${{ steps.proving.outputs.mode == 'hosted' }}" in step
    assert "export-testnet-replay.mjs" in step
    assert "--deployment-sha256" in step
    assert 'deployment_sha256=$(node' in step
    assert '--deployment-journal "$RUNNER_TEMP/public-deployment-replay"' in step
    assert ".local/" not in step
    assert "private_key" not in step


def test_native_gate_uses_both_pools_and_prepublication_not_rehearsal_pins():
    step = workflow_step()
    assert "authorization.package_version !== process.env.METROPOLIS_VERSION" in step
    assert "--prepublication-authorization packaging/metropolis/testnet-release.json" in step
    assert "--rehearsal-release" not in step
    assert "--pools wmon,usdc" in step
    assert "> registry/qualification/workflow.json" in step
    assert 'cp "$RUNNER_TEMP"/proving/manifest-*.json' in step


def test_platform_compare_requires_identity_bound_feature_gates_for_hosted_relayers():
    text = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    assert 'if modes == {"hosted"} and "erebus-shielded-relayer" in manifest.get("binaries", {}):' in text
    assert "checker.check_feature_gates(" in text
    assert 'release_pin = hashlib.sha256((directory / "release.json").read_bytes()).hexdigest()' in text
    assert 'sys.exit("; ".join(failures))' in text


def test_public_replay_offline_regressions_remain_in_typescript_ci():
    text = (ROOT / ".github/workflows/ci.yml").read_text()
    typescript = text.split("  typescript:\n", 1)[1].split("  secrets:\n", 1)[0]
    assert "npm ci --prefix circuits/m5" in typescript
    assert "node --test circuits/m5/scripts/export-testnet-replay.test.mjs" in typescript
    assert "  m5-proof:\n" not in text
