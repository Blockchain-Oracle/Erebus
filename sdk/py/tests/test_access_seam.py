"""Path-only retrieval and strict separation of resource and payment evidence."""

import json
import subprocess

import pytest

from erebus import AccessError, AccessSeam


def request():
    return dict(evidence_file="deal.evidence", buyer_key_file="buyer.key", service_url="https://example.com/v1/access",
                service_id="ab" * 32, cache_root="cache")


def metered_request():
    return dict(evidence_file="deal.evidence", buyer_key_file="buyer.key", service_url="https://example.com/v1/metered",
                service_id="ab" * 32, cache_root="cache")


def response():
    return {"status": "retrieved", "result": {"deal_commitment": "ab" * 32, "issuance_id": "cd" * 32,
        "resource_sha256": "ef" * 32, "resource_bytes": 123, "resource_file": f"cache/{'cd' * 32}.0.tx",
        "cached": False, "seller_reported_payment_finalized": True, "payment_verified": False,
        "resource_verified": True, "delivery_verified": False}}


def call_response():
    return {"status": "issued", "result": {"deal_commitment": "ab" * 32, "entitlement_id": "cd" * 32,
        "call_index": 0, "quantity": 3, "next_call_index": 1, "remaining": 2, "response_sha256": "ef" * 32,
        "response_bytes": 123, "response_file": f"cache/metered/calls/{'12' * 32}.0.tx", "repeated": False,
        "cached": False, "late": False, "seller_reported_payment_finalized": True,
        "response_hash_consistent": True, "resource_verified": False, "payment_verified": False,
        "delivery_verified": False}}


def status_response():
    return {"status": "allowance", "result": {"deal_commitment": "ab" * 32, "entitlement_id": "cd" * 32,
        "quantity": 3, "seller_next_call_index": 1, "remaining": 2, "next_call_index": 1, "in_flight": None,
        "exhausted": False, "recovered_calls": 0, "seller_reported_payment_finalized": True,
        "payment_verified": False, "delivery_verified": False}}


def fake(monkeypatch, value, code=0, stderr=""):
    seen = []
    def run(args, **kwargs):
        seen.append((args, kwargs))
        return subprocess.CompletedProcess(args, code, json.dumps(value), stderr)
    monkeypatch.setattr(subprocess, "run", run)
    return seen


def test_paths_only_and_no_second_payment_request(monkeypatch):
    seen = fake(monkeypatch, response())
    result = AccessSeam(binary="access").retrieve(**request())
    assert result == response()
    args, params = seen[0]
    assert args == ["access"]
    assert json.loads(params["input"]) == {**request(), "method": "retrieve", "allow_loopback_http": False}
    assert result["result"]["resource_verified"] and not result["result"]["payment_verified"]


@pytest.mark.parametrize("path", ["/private-provider-marker", "cache/../private-provider-marker", f"cache/{'ef' * 32}.0.tx"])
def test_snapshot_result_cannot_redirect_to_another_file(monkeypatch, path):
    value = response()
    value["result"]["resource_file"] = path
    fake(monkeypatch, value)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").retrieve(**request())
    assert "private-provider-marker" not in str(error.value)


@pytest.mark.parametrize("path", ["/private-provider-marker", "cache/metered/calls/../private-provider-marker", "cache/metered/calls/private-provider-marker.0.tx"])
def test_metered_result_cannot_redirect_outside_the_call_cache(monkeypatch, path):
    value = call_response()
    value["result"]["response_file"] = path
    fake(monkeypatch, value)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").metered_call(**metered_request())
    assert "private-provider-marker" not in str(error.value)


def test_x402_is_explicit_and_keeps_authorization_out_of_python(monkeypatch):
    seen = fake(monkeypatch, response())
    AccessSeam(binary="access").retrieve(**request(), x402_exact=True)
    payload = json.loads(seen[0][1]["input"])
    assert payload["x402_exact"] is True
    assert "signature" not in payload and "payment" not in payload


@pytest.mark.parametrize("value", [1, "true", None])
def test_x402_mode_requires_a_boolean(monkeypatch, value):
    seen = fake(monkeypatch, response())
    with pytest.raises(AccessError):
        AccessSeam(binary="access").retrieve(**request(), x402_exact=value)
    assert not seen


def test_pending_preserves_uncertainty(monkeypatch):
    pending = {"status": "pending", "retry_without_payment": True, "payment_verified": False,
               "resource_verified": False, "delivery_verified": False}
    fake(monkeypatch, pending, 2)
    assert AccessSeam(binary="access").retrieve(**request()) == pending
    pending["payment_verified"] = True
    fake(monkeypatch, pending, 2)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").retrieve(**request())


def test_paid_but_undelivered_is_a_seller_claim_not_chain_evidence(monkeypatch):
    report = {"status":"paid_but_undelivered", "seller_reported_payment_finalized":True,
              "retry_without_payment":True, "payment_verified":False, "resource_verified":False, "delivery_verified":False}
    fake(monkeypatch, report, 2)
    assert AccessSeam(binary="access").retrieve(**request()) == report
    report["payment_verified"] = True
    fake(monkeypatch, report, 2)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").retrieve(**request())


def test_protocol_without_keys_or_service(monkeypatch):
    seen = fake(monkeypatch, {"status": "ok", "protocol": 2, "methods": ["retrieve", "metered_call", "metered_status"]})
    assert AccessSeam(binary="access").version()["protocol"] == 2
    assert seen[0][0] == ["access", "--version"]
    assert seen[0][1]["input"] is None


def test_a_protocol_1_binary_is_refused(monkeypatch):
    fake(monkeypatch, {"status": "ok", "protocol": 1, "methods": ["retrieve"]})
    with pytest.raises(AccessError):
        AccessSeam(binary="access").version()


def test_metered_call_marshals_paths_and_optional_index(monkeypatch):
    seen = fake(monkeypatch, call_response())
    result = AccessSeam(binary="access").metered_call(**metered_request(), call_index=2)
    assert result == call_response()
    args, params = seen[0]
    assert args == ["access"]
    assert json.loads(params["input"]) == {**metered_request(), "method": "metered_call",
                                           "allow_loopback_http": False, "call_index": 2}


def test_metered_call_without_an_index_leaves_it_absent(monkeypatch):
    seen = fake(monkeypatch, call_response())
    AccessSeam(binary="access").metered_call(**metered_request())
    assert "call_index" not in json.loads(seen[0][1]["input"])


@pytest.mark.parametrize("value", [-1, 2**64, True, "0", 1.0])
def test_metered_call_index_must_be_an_unsigned_integer(monkeypatch, value):
    seen = fake(monkeypatch, call_response())
    with pytest.raises(AccessError):
        AccessSeam(binary="access").metered_call(**metered_request(), call_index=value)
    assert not seen


def test_metered_status_marshals_paths_only(monkeypatch):
    seen = fake(monkeypatch, status_response())
    result = AccessSeam(binary="access").metered_status(**metered_request())
    assert result == status_response()
    assert json.loads(seen[0][1]["input"]) == {**metered_request(), "method": "metered_status",
                                               "allow_loopback_http": False}


def test_metered_pending_preserves_uncertainty(monkeypatch):
    pending = {"status": "pending", "retry_without_payment": True, "payment_verified": False,
               "resource_verified": False, "delivery_verified": False}
    fake(monkeypatch, pending, 2)
    assert AccessSeam(binary="access").metered_call(**metered_request()) == pending
    pending["payment_verified"] = True
    fake(monkeypatch, pending, 2)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").metered_call(**metered_request())


@pytest.mark.parametrize("field,value", [("payment_verified", True), ("delivery_verified", True), ("resource_verified", True),
                                         ("response_hash_consistent", False), ("seller_reported_payment_finalized", False),
                                         ("response_file", ""), ("response_sha256", "not-a-hash"), ("call_index", -1),
                                         ("quantity", "3"), ("repeated", 1)])
def test_malformed_metered_call_metadata_is_rejected(monkeypatch, field, value):
    output = call_response()
    output["result"][field] = value
    fake(monkeypatch, output)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").metered_call(**metered_request())


@pytest.mark.parametrize("field,value", [("payment_verified", True), ("delivery_verified", True),
                                         ("seller_reported_payment_finalized", False), ("in_flight", "1"),
                                         ("recovered_calls", -1), ("exhausted", 1)])
def test_malformed_metered_status_metadata_is_rejected(monkeypatch, field, value):
    output = status_response()
    output["result"][field] = value
    fake(monkeypatch, output)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").metered_status(**metered_request())


@pytest.mark.parametrize("field,value", [("evidence_file", ""), ("buyer_key_file", {}), ("service_id", "0" * 64), ("service_id", "AB" * 32), ("allow_loopback_http", 1)])
def test_bad_input_never_starts_a_process(monkeypatch, field, value):
    seen = fake(monkeypatch, response())
    params = request()
    params[field] = value
    with pytest.raises(AccessError):
        AccessSeam(binary="access").retrieve(**params)
    assert not seen


@pytest.mark.parametrize("field,value", [("resource_bytes", True), ("resource_bytes", 1024 * 1024 + 1), ("resource_file", ""), ("resource_sha256", "not-a-hash"), ("cached", 1), ("payment_verified", True), ("delivery_verified", True), ("resource_verified", False), ("seller_reported_payment_finalized", False), ("payload_hex", "private-dataset")])
def test_malformed_or_private_metadata_is_rejected(monkeypatch, field, value):
    output = response()
    output["result"][field] = value
    fake(monkeypatch, output)
    with pytest.raises(AccessError):
        AccessSeam(binary="access").retrieve(**request())


@pytest.mark.parametrize("code,stderr", [(1, ""), (0, "private-provider-error")])
def test_process_errors_are_redacted(monkeypatch, code, stderr):
    fake(monkeypatch, {"status": "error", "error": "private-provider-error"}, code, stderr)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").retrieve(**request())
    assert "private-provider-error" not in str(error.value)


def test_hard_failure_preserves_the_rust_error_code(monkeypatch):
    failure = {"status": "error", "error": "seller ledger is behind the local cursor; stop and report it, do not pay again",
               "error_code": "seller_ledger_behind", "retry_without_payment": True}
    fake(monkeypatch, failure, 1)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").metered_call(**metered_request())
    assert error.value.code == "seller_ledger_behind"
    assert "seller ledger" in str(error.value)


def test_snapshot_failure_without_a_code_stays_a_plain_error(monkeypatch):
    fake(monkeypatch, {"status": "error", "error": "access service unavailable", "retry_without_payment": True}, 1)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").retrieve(**request())
    assert error.value.code is None


@pytest.mark.parametrize("code", [None, "seller_ledger_behind", "allowance_exhausted", "unrecognized_private_code"])
def test_valid_failure_envelope_never_forwards_free_form_private_details(monkeypatch, code):
    failure = {"status": "error", "error": "private-provider-marker", "retry_without_payment": True}
    if code is not None:
        failure["error_code"] = code
    fake(monkeypatch, failure, 1)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").metered_call(**metered_request())
    assert "private-provider-marker" not in str(error.value)
    assert "unrecognized_private_code" not in str(error.value)
    assert error.value.code == (code if code in {"seller_ledger_behind", "allowance_exhausted"} else None)


def test_undecodable_process_output_is_redacted(monkeypatch):
    def run(*args, **kwargs):
        raise UnicodeDecodeError("utf-8", b"private-provider-marker\xff", 23, 24, "invalid byte")
    monkeypatch.setattr(subprocess, "run", run)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").retrieve(**request())
    assert "private-provider-marker" not in str(error.value)
    assert "retry without another payment" in str(error.value)


@pytest.mark.parametrize("failure", [
    {"status": "error", "error": "x"},
    {"status": "error", "error": "", "retry_without_payment": True},
    {"status": "error", "error": "x", "retry_without_payment": True, "error_code": "NOT-A-CODE"},
    {"status": "error", "error": "x", "retry_without_payment": True, "private": "leak"},
])
def test_malformed_hard_failure_bodies_are_rejected(monkeypatch, failure):
    fake(monkeypatch, failure, 1)
    with pytest.raises(AccessError) as error:
        AccessSeam(binary="access").metered_call(**metered_request())
    assert error.value.code is None
    assert "leak" not in str(error.value)
