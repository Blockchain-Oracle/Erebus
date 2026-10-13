"""Grant-funded work: validation, rule ordering, exhaustion, expiry, and the summary."""

import pytest

from erebus_agents.firewall import ALLOWED, COUNTERPARTY_NOT_ALLOWED, PER_DEAL_EXCEEDED
from erebus_agents.grant import (
    GRANT_EXHAUSTED,
    GRANT_EXPIRED,
    TOKEN_NOT_GRANTED,
    BudgetGrant,
    GrantLedger,
    authorize,
    charge,
    summarize,
)

AT = 1_000_000
TOKEN = "0xusdc"
GRANT = BudgetGrant("g", "0xsponsor", "0xagent", "0xusdc", max_total=1000, expires_at=AT + 3600,
                    per_deal=400, allowed_counterparties=frozenset({"0xa", "0xb"}))


def test_grant_validation():
    with pytest.raises(ValueError):
        BudgetGrant("", "0xs", "0xa", "0xu", 1, AT + 1)
    with pytest.raises(ValueError):
        BudgetGrant("g", "0xs", "0xa", "0xu", 0, AT + 1)
    with pytest.raises(ValueError):
        BudgetGrant("g", "0xs", "0xa", "0xu", 1, AT + 1, per_deal=0)


def test_rule_ordering_expiry_then_counterparty_then_per_deal_then_budget():
    ledger = GrantLedger()
    assert authorize(GRANT, ledger, token=TOKEN, amount=10_000, counterparty="0xc", at=AT).code == COUNTERPARTY_NOT_ALLOWED
    assert authorize(GRANT, ledger, token=TOKEN, amount=500, counterparty="0xa", at=AT).code == PER_DEAL_EXCEEDED
    ledger.record("g", 900)
    assert authorize(GRANT, ledger, token=TOKEN, amount=200, counterparty="0xa", at=AT).code == GRANT_EXHAUSTED
    assert authorize(GRANT, ledger, token=TOKEN, amount=100, counterparty="0xa", at=GRANT.expires_at + 1).code == GRANT_EXPIRED


def test_charge_accumulates_and_summarizes():
    ledger = GrantLedger()
    assert charge(GRANT, ledger, token=TOKEN, amount=300, counterparty="0xa", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, token=TOKEN, amount=300, counterparty="0xb", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, token=TOKEN, amount=300, counterparty="0xa", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, token=TOKEN, amount=200, counterparty="0xa", at=AT).code == GRANT_EXHAUSTED
    summary = summarize(GRANT, ledger, draws=3)
    assert summary.spent == 900 and summary.remaining == 100 and summary.draws == 3
    assert charge(GRANT, ledger, token=TOKEN, amount=100, counterparty="0xa", at=AT).code == ALLOWED


def test_authorize_rejects_bad_amount():
    with pytest.raises(ValueError):
        authorize(GRANT, GrantLedger(), token=TOKEN, amount=0, counterparty="0xa", at=AT)


def test_a_draw_in_another_token_is_refused_not_counted():
    ledger = GrantLedger()
    decision = charge(GRANT, ledger, token="0xweth", amount=100, counterparty="0xa", at=AT)
    assert not decision.allowed and decision.code == TOKEN_NOT_GRANTED
    assert ledger.spent("g") == 0


def test_token_and_counterparty_match_case_insensitively():
    checksummed = BudgetGrant("g2", "0xs", "0xagent", "0xUSDC", max_total=100, expires_at=AT + 1,
                              allowed_counterparties=frozenset({"0xAbC"}))
    assert authorize(checksummed, GrantLedger(), token="0xusdc", amount=1, counterparty="0xabc", at=AT).allowed
    assert authorize(checksummed, GrantLedger(), token="0xUsDc", amount=1, counterparty="0xABC", at=AT).allowed


def test_ledger_refuses_a_nonpositive_record():
    with pytest.raises(ValueError):
        GrantLedger().record("g", 0)
