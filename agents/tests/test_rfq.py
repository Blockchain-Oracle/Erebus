"""RFQ desk: sealed comparison, award selection, and the mock rehearsal."""

import asyncio

import pytest

from erebus_agents.rfq import AwardStatus, Quote, RequestForQuote, award
from erebus_agents.rfq_demo import default_quotes, rehearse

AT = 1_000_000
RFQ = RequestForQuote("rfq-1", "gpu-hours", quantity=100, max_unit_price=12, deadline=AT + 3600)


def test_request_and_quote_validation():
    with pytest.raises(ValueError):
        RequestForQuote("", "item", 1, 1, AT + 1)
    with pytest.raises(ValueError):
        RequestForQuote("rfq", "item", 0, 1, AT + 1)
    with pytest.raises(ValueError):
        RequestForQuote("rfq", "item", 1, 1, 0)
    with pytest.raises(ValueError):
        Quote("0xa", "rfq", 0, AT + 1)
    assert RFQ.ceiling == 1200 and Quote("0xa", "rfq", 10, AT + 1).total(100) == 1000


def test_award_picks_the_cheapest_valid_quote_and_explains_the_rest():
    quotes = (
        Quote("0xb", "rfq-1", 10, AT + 60),
        Quote("0xa", "rfq-1", 9, AT + 60),
        Quote("0xc", "rfq-1", 13, AT + 60),
        Quote("0xd", "rfq-1", 8, AT - 1),
    )
    result = award(RFQ, quotes, at=AT)
    assert result.status is AwardStatus.AWARDED
    assert result.winner is not None and result.winner.supplier == "0xa"
    reasons = dict(result.rejected)
    assert reasons == {"0xb": "outbid", "0xc": "unit price above the buyer's ceiling", "0xd": "quote expired"}


def test_award_ties_break_on_supplier_and_ignores_other_requests():
    quotes = (Quote("0xb", "rfq-1", 9, AT + 60), Quote("0xa", "rfq-1", 9, AT + 60),
              Quote("0xc", "rfq-2", 1, AT + 60))
    result = award(RFQ, quotes, at=AT)
    assert result.winner is not None and result.winner.supplier == "0xa"
    assert ("0xc", "quote is for another request") in result.rejected


def test_award_reports_no_valid_quote():
    result = award(RFQ, (Quote("0xa", "rfq-1", 13, AT + 60), Quote("0xb", "rfq-1", 1, AT - 1)), at=AT)
    assert result.status is AwardStatus.NO_VALID_QUOTE and result.winner is None


def test_rehearsal_settles_with_the_awarded_supplier(tmp_path):
    record = asyncio.run(rehearse(RFQ, default_quotes(RFQ, AT), at=AT, store_dir=tmp_path))
    assert record["settled"] and record["award"]["winner"] == "0xatlas"
    assert record["paid_amount"] == record["agreed_amount"] <= record["ceiling"]
    assert record["audit"]["compliant"]


def test_rehearsal_stops_when_no_quote_is_valid(tmp_path):
    over = tuple(Quote(supplier, RFQ.rfq_id, 20, AT + 60)
                 for supplier in ("0xa", "0xb"))
    record = asyncio.run(rehearse(RFQ, over, at=AT, store_dir=tmp_path))
    assert record["award"]["status"] == "no_valid_quote" and record["settled"] is False
