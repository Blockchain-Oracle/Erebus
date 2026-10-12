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
