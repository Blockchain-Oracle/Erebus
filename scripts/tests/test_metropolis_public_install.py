"""The public install check proves the same transitions as the release gate and only over HTTPS."""

import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


public = load("metropolis_public_install", "scripts/check-metropolis-public-install.py")
checker = load("metropolis_public_checker", "scripts/check-metropolis-install.py")


def test_public_install_proves_the_same_public_inputs_as_installed_qualification():
    assert public.TRANSITIONS == checker.TRANSITIONS


@pytest.mark.parametrize("url", ["http://example.net/x", "file:///etc/passwd", "ftp://example.net/x"])
def test_downloads_refuse_anything_but_https(url):
    with pytest.raises(ValueError, match="https"):
        public.https(url)


def test_unsupported_platforms_are_refused(monkeypatch):
    monkeypatch.setattr(public.platform, "system", lambda: "Windows")
    with pytest.raises(ValueError, match="macOS arm64 and Linux x86_64"):
        public.wheel_tag()


def test_the_environment_drops_erebus_uv_and_python_overrides(tmp_path, monkeypatch):
    for key in ("EREBUS_BACKEND", "UV_INDEX_URL", "PIP_INDEX_URL", "VIRTUAL_ENV", "PYTHONPATH"):
        monkeypatch.setenv(key, "x")
    environment = public.clean_environment(tmp_path)
    assert not any(key.startswith(("EREBUS_", "PIP_", "VIRTUAL_ENV", "PYTHON")) for key in environment)
    assert environment["UV_CACHE_DIR"] == str(tmp_path / "uv-cache")


def test_the_workflow_runs_the_check_on_both_native_platforms_without_publishing():
    text = (ROOT / ".github/workflows/metropolis-public-install.yml").read_text()
    assert "macos-14" in text and "ubuntu-latest" in text and "workflow_dispatch" in text
    assert "permissions:\n  contents: read\n" in text and ": write" not in text
    assert "on:\n  workflow_dispatch:\n    inputs:\n      version:" in text and "push:" not in text
    for forbidden in ("gh release", "gh api", "secrets.", "mapfile", "git push"):
        assert forbidden not in text


def test_the_registry_workflow_can_run_only_the_public_install_check():
    text = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    assert "uses: ./.github/workflows/metropolis-public-install.yml" in text
    assert "if: ${{ inputs.public_install_version == '' || inputs.public_install_version == null }}" in text
    assert "workflow_call:" in (ROOT / ".github/workflows/metropolis-public-install.yml").read_text()


def compare_script() -> str:
    text = (ROOT / ".github/workflows/metropolis-registry.yml").read_text()
    body = text.split("      - name: Same commit, clean source, identical pure-Python wheels", 1)[1]
    body = body.split("python3 - <<'PY'\n", 1)[1].split("\n          PY\n", 1)[0]
    return "\n".join(line[10:] for line in body.splitlines())


def compare_leg(root, platform, mode, manifest):
    leg = root / "artifacts" / f"metropolis-registry-{platform}"
    (leg / "qualification").mkdir(parents=True)
    (leg / "proving-mode").write_text(mode + "\n")
    (leg / "release.json").write_text(json.dumps({
        "platform": platform, "source_commit": "c" * 40, "dirty_source": False,
        "candidate": {"source_tree_sha256": "t" * 64, "release_eligible": True},
        "proving_domains": [{"manifest_sha256": manifest}], "wheels": {"erebus_sdk-1-py3-none-any.whl": {"sha256": "w"}}}))
    hosted = [{"source_manifest_sha256": manifest, "release_recorded": True, "proved": True, "cache_tamper": "redownloaded",
               "authorization_refusals": {"wrong_publisher": True}}]
    reports = {"install": {"installed_onboarding": {}, "installed_hosted_artifacts": hosted},
               "assets": {"installed_asset_rehearsal": True}, "metered": {"installed_metered_rehearsal": True},
               "x402": {"installed_x402_rehearsal": True}}
    for gate, report in reports.items():
        (leg / "qualification" / f"{gate}.json").write_text(json.dumps({"status": "verified", "source_imports": False, **report}))


@pytest.mark.parametrize("mode,manifests,passes", [
    ("ci-fixture", ("a" * 64, "b" * 64), True),   # each leg builds its own throwaway fixture bundle
    ("hosted", ("a" * 64, "b" * 64), False),      # hosted inputs are shared and must match
    ("hosted", ("a" * 64, "a" * 64), True),
])
def test_compare_checks_fixture_legs_individually_and_hosted_legs_together(tmp_path, mode, manifests, passes):
    import subprocess
    import sys
    for platform, manifest in zip(("macos-arm64", "linux-x86_64"), manifests):
        compare_leg(tmp_path, platform, mode, manifest)
    (tmp_path / "compare.py").write_text(compare_script())
    result = subprocess.run([sys.executable, "compare.py"], cwd=tmp_path, capture_output=True, text=True)
    assert (result.returncode == 0) is passes, result.stdout + result.stderr
