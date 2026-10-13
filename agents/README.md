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
- `marketplace.py` / `marketplace_demo.py`. A capability marketplace on the real Metropolis
  loop. Sellers publish a `ServiceListing` into a file-backed `ServiceRegistry`; the buyer's
  operator keeps a route file (`PurchaseRoute`) mapping each listing to the configured servers for
  that seller, since each negotiation file pins exactly one peer. The buyer picks the cheapest
  routed listing inside its ceiling and buys it through `metropolis_loop.run_over_mcp`: negotiate,
  settle at most once, recover by observation until finalized, and retrieve the resource when
  access is configured. The advertised band only chooses the seller; the agreed price is fixed by
  operator files and capped natively by the buyer's `maximum_price`. Delivery state
  (`PENDING`/`DELIVERED`/`REFUNDED`/`DISPUTED`) is kept apart from settlement on purpose: the
  protocol never proves delivery.
- `api_gateway_demo.py`. Pay per call over the operator-selected `x402-exact` rail: one deal per
  call through the participants' real MCP servers (`metropolis_loop.drive_x402_deal`), so every
  retry is a retrieval reusing the retained permit. Operation IDs are written to an owner-only
  ledger before the first call, and an interrupted run resumes from the same ledger. Standard
  x402 is public, never private; payment is verified only by an independent auditor.
- `receipt_explorer.py`. Renders one disclosed deal as a private receipt from a
  `metropolis_audit.py` report, and nothing else. It keeps agreement, payment and delivery
  distinct, refuses a receipt that claims delivery verification, and exports text, JSON or a
  self-contained static HTML file. Also a CLI.
- `rfq.py` / `rfq_demo.py`. A confidential request-for-quote desk: one buyer issues a
  `RequestForQuote` with a hidden ceiling, suppliers return sealed `Quote`s, and `award` picks the
  cheapest valid quote (rejecting expired, over-ceiling and outbid ones, and everything once the
  request deadline passes) without revealing the losers. The winner is then negotiated and
  settled through the ordinary loop with the awarded quote as the buyer's budget, so it is paid
  at most what it quoted (`award_honored`).
- `vendor_selection.py` / `vendor_selection_demo.py`. `negotiate_agreement` runs a private
  negotiation with each candidate seller *without settling*, and `select` picks the cheapest
  agreement at or below the ceiling. `settle_selected` then settles that exact offer and checks
  the payment equals the compared amount. The buyer pays exactly one seller; losing candidates
  are never paid.
- `firewall.py` / `firewall_demo.py`. The agent-side spending policy: per-token per-deal and daily
  caps, a `SpendLedger`, and the counterparty allowlist the MCP `SpendGuard` does not have.
  Token and counterparty identities match case-insensitively. It is not wired into the
  settlement path: an agent must call `guard`/`charge` itself, and `SpendGuard` remains the only
  limit enforced on a settlement.
- `grant.py` / `grant_demo.py`. A sponsor's `BudgetGrant` (token, total budget, per-deal cap,
  counterparty allowlist, expiry) and a `GrantLedger`, so an agent's spending is attributable to a
  sponsor's intent. A draw in any token other than the grant's is refused. Shares the firewall's
  refusal codes.
- `discovery.py` / `discovery_demo.py`. A `DiscoveryDirectory` indexed from real signed
  `ServiceDescriptor` files (`erebus-negotiate prepare_operator` writes them). A record pairs a
  capability and an advertised price band with one descriptor file; address, endpoints, chain,
  assets, modes and expiry are read from that file, and its bytes are pinned by SHA-256.
  `resolve` refuses an expired or swapped descriptor. It indexes; Rust verifies the signature when
  the buyer operator configures the file as its `peer_descriptor_file`. Also a CLI.
- `metering.py` / `saas_metering_demo.py`. Prepaid metered access on the real metered profile:
  one settlement (`drive_deal`) buys the bundle, then `call_metered_service` spends calls against
  the seller's allowance from `metered_service_status`. A pending call is retried by calling
  again, never by paying, and nothing is called past exhaustion. Rust keeps the cursor and the
  seller's issuer is authoritative; `--consume-only` spends from a bundle already paid.
- `offchain_purchase_demo.py`. Compute, dataset or API purchases: the marketplace purchase under
  a kind-specific capability, with the same registry, routes and operation ID.
- `demo.py` / `demo_mcp.py`, the two CLI entry points above.

```bash
# Marketplace: buy the cheapest routed listing through the real Metropolis servers
uv run python agents/src/erebus_agents/marketplace_demo.py --registry registry.json \
  --routes buyer-routes.json --capability summarize-documents --ceiling-unit-price 8 \
  --operation <64-hex operation ID; reuse it after any interruption>

# Paid API gateway: per-call access over the public x402-exact rail (not private)
uv run python agents/src/erebus_agents/api_gateway_demo.py --buyer-negotiation buyer.json \
  --buyer-access buyer-access.json --seller-negotiation seller.json --calls 3 --ledger calls.json

# Receipt explorer: format the report metropolis_audit.py produced
uv run python agents/src/erebus_agents/receipt_explorer.py --report audit.json \
  --issuer <participant> --format text   # or --format html --out receipt.html

# Discovery: index a seller's signed descriptor, then search and resolve
uv run python agents/src/erebus_agents/discovery_demo.py --directory discovery.json publish \
  --service-id svc-atlas --capability summarize-documents --min-unit-price 3 --max-unit-price 7 \
  --descriptor /operator/seller/keys/seller.descriptor.json
uv run python agents/src/erebus_agents/discovery_demo.py --directory discovery.json search \
  --capability summarize-documents --mode shielded

# SaaS metering: one settlement buys the bundle, then spend calls (metered access profile)
uv run python agents/src/erebus_agents/saas_metering_demo.py --buyer-negotiation buyer.json \
  --buyer-payment payment.json --buyer-access buyer-metered-access.json \
  --seller-negotiation seller.json --operation <64-hex> --calls 5

# Off-chain purchase: the marketplace purchase for compute, a dataset or an API response
uv run python agents/src/erebus_agents/offchain_purchase_demo.py --kind compute \
  --registry registry.json --routes buyer-routes.json --ceiling-unit-price 8 --operation <64-hex>

# RFQ desk, vendor selection, spending firewall and grants (not yet on the real path)
uv run python agents/src/erebus_agents/rfq_demo.py --latency 0
uv run python agents/src/erebus_agents/vendor_selection_demo.py --latency 0
uv run python agents/src/erebus_agents/firewall_demo.py
uv run python agents/src/erebus_agents/grant_demo.py
```

## Where it sits

```
reference policy rehearsal → mock ErebusClient
reference policy rehearsal (real transport) → MCP → mock/seam ErebusClient
external autonomous agents → MCP → sdk/py → sdk/rs → Starknet
```

Built against the Protocol 4 MCP interface in ARCHITECTURE §4, not the mock internals. The
same loop uses the mock or the real seam without changing operation-ID behavior.
