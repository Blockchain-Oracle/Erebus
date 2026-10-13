"""Spending firewall: caps, allowlist ordering, and the daily ledger."""

import pytest

from erebus_agents.firewall import (
    ALLOWED,
    COUNTERPARTY_NOT_ALLOWED,
    DAILY_EXCEEDED,
    PER_DEAL_EXCEEDED,
    SpendingPolicy,
    SpendLedger,
    TokenLimit,
    charge,
    evaluate,
    guard,
)

TOKEN = "0xusdc"
DAY = "2026-10-13"
A, B, C = "0xa", "0xb", "0xc"
POLICY = SpendingPolicy(by_token={TOKEN: TokenLimit(per_deal=500, daily=1000)},
                        allowed_counterparties=frozenset({A, B}))


def test_evaluate_rejects_bad_arguments():
    with pytest.raises(ValueError):
        evaluate(POLICY, token=TOKEN, amount=0, counterparty=A)
    with pytest.raises(ValueError):
        evaluate(POLICY, token="", amount=1, counterparty=A)


def test_empty_allowlist_allows_every_counterparty():
    open_policy = SpendingPolicy(by_token={TOKEN: TokenLimit(per_deal=500)})
    assert evaluate(open_policy, token=TOKEN, amount=1, counterparty=C).allowed


def test_counterparty_is_checked_before_amounts():
    decision = evaluate(POLICY, token=TOKEN, amount=10_000, counterparty=C)
    assert not decision.allowed and decision.code == COUNTERPARTY_NOT_ALLOWED


def test_per_deal_and_daily_caps():
    assert evaluate(POLICY, token=TOKEN, amount=501, counterparty=A).code == PER_DEAL_EXCEEDED
    assert evaluate(POLICY, token=TOKEN, amount=400, counterparty=A, committed_today=700).code == DAILY_EXCEEDED
    assert evaluate(POLICY, token=TOKEN, amount=300, counterparty=A, committed_today=700).allowed


def test_ledger_accumulates_only_allowed_spends():
    ledger = SpendLedger()
    assert charge(POLICY, ledger, token=TOKEN, amount=400, counterparty=A, day=DAY).allowed
    assert charge(POLICY, ledger, token=TOKEN, amount=400, counterparty=B, day=DAY).allowed
    assert charge(POLICY, ledger, token=TOKEN, amount=300, counterparty=A, day=DAY).code == DAILY_EXCEEDED
    assert charge(POLICY, ledger, token=TOKEN, amount=10, counterparty=C, day=DAY).code == COUNTERPARTY_NOT_ALLOWED
    assert ledger.spent(DAY, TOKEN) == 800
    assert ledger.spent("2026-10-14", TOKEN) == 0
    assert guard(POLICY, ledger, token=TOKEN, amount=300, counterparty=A, day=DAY).code == DAILY_EXCEEDED
    assert guard(POLICY, ledger, token=TOKEN, amount=100, counterparty=A, day="2026-10-14").code == ALLOWED
