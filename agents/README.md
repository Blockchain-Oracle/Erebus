# Reference agents

Owned by Ishita (CLAUDE.md, repo layout). Two agents, a buyer and a seller, running the
offer/counter/accept loop. Two ways to run them:

**`demo.py`** talks directly to a mock of `ErebusClient` — fast, deterministic rehearsal,
no MCP transport involved (`erebus_mcp.interface`/`erebus_mcp.mock_client`).

**`demo_mcp.py`** runs the same policies as real MCP clients against three live
`server.py` subprocesses (buyer, seller, auditor), over stdio. This is the "any framework
can drive it" claim exercised for real, not simulated.

```bash
uv sync --all-packages
uv run python agents/src/erebus_agents/demo.py       # mock rehearsal
uv run python agents/src/erebus_agents/demo_mcp.py    # real MCP servers
```

`demo.py` takes `--latency SECONDS` (default 0.2; real proof latency is ~29s per round,
pass `--latency 29` to rehearse timing). Both take `--rounds`, `--budget`, `--reserve`.

## What's here

- `policy.py`. `BuyerPolicy` / `SellerPolicy`. Pure, deterministic decision logic, no I/O.
Threshold rules only. The buyer only names amounts at or below its spendable total (settlement
covers the price and returns change); the seller never accepts because the accepting identity
pays, and confirms agreement by countering at the buyer's amount so the buyer can accept a
seller-authored offer.
- `agent.py`. `run_negotiation()`: same loop against `MockErebusClient` directly, plus
  the auditor reveal.
- `mcp_loop.py`. `run_negotiation_over_mcp()`: the same loop, driven by real MCP tool
  calls over three subprocess servers instead of direct mock calls. It persists canonical
  write intent before each call. After an interruption, it reconciles and reuses the
  original operation ID. It stops when Rust reports ambiguity or operator action.
- `intents.py`. Durable mode-`0600` agent intent records. Each record binds one operation ID
  to one canonical MCP write before the call starts.
- `procurement.py`. A buyer's `Requisition`, a supplier's `Catalog`, and `review_disclosure`,
  the auditor's check that a disclosed deal settled, paid what was agreed, and stayed under the
  ceiling. `procurement_demo.py` rehearses it on the mock client.
- `metropolis_audit.py`. The real disclosure step over the disclosure MCP servers: an issuer
  selects and exports one deal to the auditor's key, the auditor verifies the agreement and polls
  the payment check. It never reports delivery as verified. Evidence and grant files are
  create-new, so re-issuing takes fresh `--evidence-name`/`--grant-name` values. Also a CLI.
- `marketplace.py` / `marketplace_demo.py`. A capability marketplace: a seller publishes a
  `ServiceListing` into a file-backed `ServiceRegistry`, a buyer searches it, picks the cheapest
  listing inside its ceiling, negotiates, pays once, and records the off-chain delivery. Delivery
  state (`PENDING`/`DELIVERED`/`REFUNDED`/`DISPUTED`) is kept apart from settlement on purpose:
  the protocol never proves delivery.
- `paid_gateway.py` / `api_gateway_demo.py`. A runnable pay-per-call rehearsal. `LocalGateway`
  models the `x402-exact` seller gateway; the demo drives the real production sequence
  (`metropolis_loop.drive_x402_deal`) over it, so each call pays once and every retry reuses the
  retained permit. For the installed rail, stand up `erebus-access-service` and drive it with
  `metropolis_loop.py --profile x402-exact`. Standard x402 is public, never private.
- `receipt_explorer.py`. Renders one disclosed deal as a private receipt, from an auditor report
  or a rehearsal. It keeps agreement, payment and delivery distinct, refuses a receipt that claims
  delivery verification, and exports text, JSON or a self-contained static HTML file. Also a CLI.
- `demo.py` / `demo_mcp.py`, the two CLI entry points above.

```bash
# Marketplace: search, negotiate, settle, deliver, audit (mock rehearsal)
uv run python agents/src/erebus_agents/marketplace_demo.py --latency 0

# Paid API gateway: per-call private access over the x402-exact sequencing
uv run python agents/src/erebus_agents/api_gateway_demo.py --calls 3

# Receipt explorer: format a verification result an auditor already produced
uv run python agents/src/erebus_agents/receipt_explorer.py --report audit.json \
  --issuer <participant> --format text   # or --format html --out receipt.html
```

## Where it sits

```
reference policy rehearsal → mock ErebusClient
reference policy rehearsal (real transport) → MCP → mock/seam ErebusClient
external autonomous agents → MCP → sdk/py → sdk/rs → Starknet
```

Built against the Protocol 4 MCP interface in ARCHITECTURE §4, not the mock internals. The
same loop uses the mock or the real seam without changing operation-ID behavior.
