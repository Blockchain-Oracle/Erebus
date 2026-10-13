"""Grant-funded work: validation, rule ordering, exhaustion, expiry, and the summary."""

import pytest

from erebus_agents.firewall import ALLOWED, COUNTERPARTY_NOT_ALLOWED, PER_DEAL_EXCEEDED
from erebus_agents.grant import GRANT_EXHAUSTED, GRANT_EXPIRED, BudgetGrant, GrantLedger, authorize, charge, summarize

AT = 1_000_000
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
    assert authorize(GRANT, ledger, amount=10_000, counterparty="0xc", at=AT).code == COUNTERPARTY_NOT_ALLOWED
    assert authorize(GRANT, ledger, amount=500, counterparty="0xa", at=AT).code == PER_DEAL_EXCEEDED
    ledger.record("g", 900)
    assert authorize(GRANT, ledger, amount=200, counterparty="0xa", at=AT).code == GRANT_EXHAUSTED
    assert authorize(GRANT, ledger, amount=100, counterparty="0xa", at=GRANT.expires_at + 1).code == GRANT_EXPIRED


def test_charge_accumulates_and_summarizes():
    ledger = GrantLedger()
    assert charge(GRANT, ledger, amount=300, counterparty="0xa", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, amount=300, counterparty="0xb", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, amount=300, counterparty="0xa", at=AT).code == ALLOWED
    assert charge(GRANT, ledger, amount=200, counterparty="0xa", at=AT).code == GRANT_EXHAUSTED
    summary = summarize(GRANT, ledger, draws=3)
    assert summary.spent == 900 and summary.remaining == 100 and summary.draws == 3
    assert charge(GRANT, ledger, amount=100, counterparty="0xa", at=AT).code == ALLOWED


def test_authorize_rejects_bad_amount():
    with pytest.raises(ValueError):
        authorize(GRANT, GrantLedger(), amount=0, counterparty="0xa", at=AT)
