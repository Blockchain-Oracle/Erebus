"""Private SaaS metering: bundle accounting, exhaustion, and the mock rehearsal."""

import asyncio

import pytest

from erebus_agents.metering import BundleExhausted, CreditBundle, consume
from erebus_agents.saas_metering_demo import rehearse

BUNDLE = CreditBundle("ent-1", "0xseller", "0xbuyer", calls_total=5)


def test_bundle_validation_and_remaining():
    with pytest.raises(ValueError):
        CreditBundle("", "0xs", "0xb", 5)
    with pytest.raises(ValueError):
        CreditBundle("e", "0xs", "0xb", 0)
    with pytest.raises(ValueError):
        CreditBundle("e", "0xs", "0xb", 5, calls_used=6)
    assert BUNDLE.remaining == 5 and not BUNDLE.exhausted


def test_consume_decrements_and_refuses_overdraw():
    spent = consume(BUNDLE, 3)
    assert spent.calls_used == 3 and spent.remaining == 2
    assert consume(spent, 2).exhausted
    with pytest.raises(BundleExhausted, match="2 of 5 remaining"):
        consume(spent, 3)
    with pytest.raises(ValueError):
        consume(BUNDLE, 0)


def test_rehearsal_buys_a_bundle_and_refuses_overdraw(tmp_path):
    record = asyncio.run(rehearse(5, 4, 3, consume_calls=7, store_dir=tmp_path))
    assert record["settled"] and record["audit"]["compliant"]
    assert record["calls_consumed"] == 5
    assert record["bundle"]["remaining"] == 0 and record["bundle"]["exhausted"] is True
    assert "0 of 5 remaining" in record["refused"]


def test_rehearsal_within_bundle_never_refuses(tmp_path):
    record = asyncio.run(rehearse(5, 4, 3, consume_calls=3, store_dir=tmp_path))
    assert record["calls_consumed"] == 3 and record["refused"] is None
    assert record["bundle"]["remaining"] == 2
