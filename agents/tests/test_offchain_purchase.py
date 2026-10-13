"""Off-chain purchases: compute and dataset reuse the marketplace loop unchanged."""

import asyncio

import pytest

from erebus_agents.offchain_purchase_demo import KINDS, rehearse_purchase


@pytest.mark.parametrize("kind", ["compute", "dataset", "api"])
def test_each_kind_settles_and_audits_compliant(kind, tmp_path):
    record = asyncio.run(rehearse_purchase(kind, 8, 10, store_dir=tmp_path))
    assert record["kind"] == kind and record["matched"] and record["settled"]
    assert record["audit"]["compliant"]
    assert record["delivery"]["status"] == "delivered" and not record["delivery"]["is_settlement_evidence"]


def test_unknown_kind_is_refused(tmp_path):
    with pytest.raises(ValueError):
        asyncio.run(rehearse_purchase("teleport", 8, 10, store_dir=tmp_path))
    assert set(KINDS) == {"compute", "dataset", "api"}


def test_no_match_when_the_ceiling_is_below_every_listing(tmp_path):
    record = asyncio.run(rehearse_purchase("dataset", 1, 10, store_dir=tmp_path))
    assert record["matched"] is False
