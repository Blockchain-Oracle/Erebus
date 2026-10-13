"""Native generated-agent qualification must use public inputs and exact authorization."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


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


def test_public_replay_offline_regressions_are_in_the_m5_ci_job():
    text = (ROOT / ".github/workflows/ci.yml").read_text()
    assert "node --test scripts/export-testnet-replay.test.mjs" in text
