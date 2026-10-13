"""Both backends share the installed init entry point without repository imports."""

import pytest

from erebus_mcp.onboarding import init_main


def test_metropolis_help(capsys):
    with pytest.raises(SystemExit) as result:
        init_main(["--metropolis", "--help"])
    assert result.value.code == 0
    output = capsys.readouterr().out
    assert "pins" in output
    assert "doctor" in output
    assert "auditor" in output


def test_default_init_still_uses_starknet(monkeypatch):
    from erebus_mcp import setup

    seen = []
    monkeypatch.setattr(setup, "main", lambda argv: seen.append(argv) or 0)
    assert init_main(["--resume"]) == 0
    assert seen == [["--resume"]]


def test_metropolis_arguments_are_forwarded_without_changing_input(monkeypatch):
    from erebus_mcp.metropolis_setup import operator_init

    seen = []
    monkeypatch.setattr(operator_init, "main", lambda argv: seen.append(argv) or 0)
    args = ["--metropolis", "doctor", "--directory", "/tmp/operator", "--offline"]
    assert init_main(args) == 0
    assert seen == [args[1:]]
    assert args[0] == "--metropolis"


def test_packaged_helpers_load():
    from erebus_mcp.metropolis_setup import operator_init

    assert operator_init.load("disclosure_config").PINS
    assert callable(operator_init.load("stage_artifacts").deployment_identity)
