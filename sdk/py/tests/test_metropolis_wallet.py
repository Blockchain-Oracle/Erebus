"""Wallet policy precedes native execution; retained outcomes never trigger another call."""

import json
import subprocess

import pytest

from erebus import MetropolisError, MetropolisWallet, WalletPolicy

REF = "ab" * 32
RECIPIENT = "0x" + "12" * 20


def wallet(methods=("notes", "sync", "deposit"), **kwargs):
    return MetropolisWallet(config_file="/operator/wallet.json", binary="wallet",
        policy=WalletPolicy("100", frozenset(methods), frozenset({RECIPIENT}), **kwargs))


def response(monkeypatch, values):
    calls = []
    queue = iter(values)
    def run(args, **kwargs):
        calls.append(json.loads(kwargs["input"]))
        code, envelope = next(queue)
        text = json.dumps(envelope)
        return subprocess.CompletedProcess(args, code, text if code == 0 else "", text if code != 0 else "")
    monkeypatch.setattr(subprocess, "run", run)
    return calls


def test_deposit_requires_reference_and_policy_before_native_execution(monkeypatch):
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{"kind":"deposit", "commitment":REF}})])
    for amount, reference in [("101", REF), ("01", REF), ("1", None), ("1", "0" * 64)]:
        with pytest.raises(MetropolisError):
            wallet().execute("deposit", amount=amount, operation_ref=reference)
    assert calls == []
    result = wallet().execute("deposit", amount="70", operation_ref=REF)
    assert result["automatic_rebroadcast"] is False
    assert result["payment_verified"] is False
    assert calls == [{"method":"deposit", "config_file":"/operator/wallet.json", "amount":"70", "operation_ref":REF}]


def test_pending_stderr_is_redacted_and_never_retried(monkeypatch):
    calls = response(monkeypatch, [(2, {"status":"pending", "error":"secret-marker",
        "result":{"history_pending":True, "next_log_block":17, "through":30, "retry_without_new_payment":True}})])
    result = wallet().execute("sync")
    assert result["status"] == "pending" and result["result"]["next_log_block"] == 17
    assert "secret-marker" not in json.dumps(result)
    assert len(calls) == 1


@pytest.mark.parametrize("result", [{"spend_secret":"secret"}, {"submission":{"payment_verified":True}}, {"notes":[{"salt":"secret"}]}])
def test_private_fields_and_false_payment_claims_are_refused(monkeypatch, result):
    calls = response(monkeypatch, [(0, {"status":"ok", "result":result})])
    with pytest.raises(MetropolisError):
        wallet().execute("notes")
    assert len(calls) == 1


def test_withdrawal_is_recipient_and_note_amount_bound(monkeypatch):
    methods = ("notes", "sync", "withdraw")
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{"notes":[{
        "commitment":REF, "amount":"101", "included":True, "consumed":False, "reserved":False}]}})])
    with pytest.raises(MetropolisError, match="recipient"):
        wallet(methods).execute("withdraw", commitment=REF, recipient="0x" + "34" * 20)
    assert calls == []
    with pytest.raises(MetropolisError, match="exceeds"):
        wallet(methods).execute("withdraw", commitment=REF, recipient=RECIPIENT)
    assert len(calls) == 1 and calls[0]["method"] == "notes"


def test_replacement_uses_only_operator_fee_caps(monkeypatch):
    methods = ("notes", "sync", "replace")
    with pytest.raises(MetropolisError, match="fee caps"):
        wallet(methods)
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{"identity":REF, "replaced":True}})])
    wallet(methods, replacement_fees=("2000", "1000")).execute("replace", identity=REF)
    assert calls[0]["max_fee_per_gas"] == "2000" and calls[0]["max_priority_fee_per_gas"] == "1000"


def test_timeouts_retain_state_without_an_automatic_second_call(monkeypatch):
    calls = []
    def run(args, **kwargs):
        calls.append(args)
        raise subprocess.TimeoutExpired(args, 1, stderr="secret-marker")
    monkeypatch.setattr(subprocess, "run", run)
    with pytest.raises(MetropolisError) as error:
        wallet().execute("deposit", amount="70", operation_ref=REF)
    assert "secret-marker" not in str(error.value) and len(calls) == 1


def test_older_native_wallet_is_refused_before_agent_deposit_is_enabled(monkeypatch):
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{"protocol":1, "methods":["notes", "sync", "deposit"]}})])
    with pytest.raises(MetropolisError, match="durable deposit references"):
        wallet().version()
    assert calls == [{"method":"version"}]


def test_paired_diagnostics_require_explicit_policy_and_native_capability(monkeypatch):
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{"protocol":1,
        "methods":["notes", "sync", "status"]}})])
    with pytest.raises(MetropolisError, match="authorized"):
        wallet().execute("status")
    assert calls == []
    with pytest.raises(MetropolisError):
        wallet(("notes", "sync", "status")).version()
    assert len(calls) == 1
    calls = response(monkeypatch, [(0, {"status":"ok", "result":{
        "account":{"native_balance":"0", "asset_balance":str(2**256 - 1), "allowance":"0",
                   "anchor_block":17, "anchor_hash":REF, "anchor_timestamp":1234},
        "checkpoints":[{"source":"peer", "checkpoint_block":12, "checkpoint_hash":REF,
                        "next_log_block":13, "blocks_behind":5, "caught_up":False}],
        "notes":[], "operations":[], "payment_verified":False}})])
    result = wallet(("notes", "sync", "status")).execute("status")
    assert result["result"]["account"]["asset_balance"] == str(2**256 - 1)
    assert result["result"]["checkpoints"][0]["caught_up"] is False
    assert calls == [{"method":"status", "config_file":"/operator/wallet.json"}]
