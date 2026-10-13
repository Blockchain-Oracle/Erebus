# Erebus Technical Demo

Target: a public operating video of no more than three minutes.
Product: published Metropolis release `0.3.0.dev2026101307`.
Record the terminal and your narration, not a source-code walkthrough or slides.

## For Ishita

Use the `metropolis` branch, not `main`. On the prepared Mac, update the checkout:

```sh
cd /Users/odinson/Developer/erebus
git switch metropolis
git pull --ff-only origin metropolis
```

If Git refuses because of local changes, stop; do not reset or discard them.
Then follow "Before Recording" and run both dry runs before capturing footage.
The prepared environment and `.local` inputs below are not included in GitHub.
For another machine, arrange the qualified registry, signed manifests and public deployment replay first.
Do not transfer real wallet keys or credential-bearing RPC files for this local demonstration.

## What You Can Claim

The release passed native macOS/Linux qualification and fresh public installation.
The recording below demonstrates actual installed MCP processes operating on local Anvil,
then reads a historical shielded WMON payment from live Monad testnet.
The local workflow uses mock assets. It is not a new live Monad payment.
The historical WMON payment used source-built commands, not this release's complete workflow.

Do not claim that fresh-wallet live settlement, sponsored live settlement or cold-history catch-up is complete.
OpenCode's segmented-store implementation and tests are separate from integrated, live catch-up qualification.
Leakage tests are local evidence, not an independent audit or deployed logging review.

## Before Recording

Run from `/Users/odinson/Developer/erebus` in a separate terminal.
Use a large font, hide notifications, and keep shell tracing disabled.
Do not show keys, RPC tokens, wallet configs, grants, note openings or witnesses.
Do not show `anvil.log` or `anvil-accounts.json`; fixture account secrets can appear there.
Do not open `erebusagents.live` as the Monad demo: it currently describes the older Starknet product.

```sh
cd /Users/odinson/Developer/erebus
set +x
set -o pipefail
export ROOT="$PWD"
export V=0.3.0.dev2026101307
export B=/private/tmp/erebus-demo-public-1307/bin
export REG="$ROOT/.local/metropolis/candidate-20261010/ci-run-38431534413/metropolis-registry-macos-arm64"
export PINS="$ROOT/.local/metropolis/live-1307-20261013/pins"
export JOURNAL="$ROOT/.local/metropolis/candidate-20261010/live-deployment-journal"
```

The prepared environment at `$B` must contain the published version, not a source-built replacement.
On this Mac, a GitHub asset timeout required installation from the downloaded native release wheels.
Their wheel hashes matched the native release record. Describe this as the published release,
not as a successful fresh public-index install performed during your recording.
If it is missing, create it and install from the public index:

```sh
uv venv --python 3.11 /private/tmp/erebus-demo-public-1307
UV_HTTP_TIMEOUT=300 uv pip install --python "$B/python" --no-config \
  --index https://poulavbhowmick03.github.io/erebus-metropolis/simple/ \
  --default-index https://pypi.org/simple --index-strategy first-index \
  "erebus-mcp-server==$V"
```

If public asset downloads time out, the downloaded qualification wheels provide a local fallback:

```sh
uv pip install --python "$B/python" --no-config --find-links "$REG/wheels" \
  --default-index https://pypi.org/simple "erebus-mcp-server==$V"
```

Use only the qualified 1307 wheels in `$REG`; do not use newly built rate-control or segmented-cache binaries.

Verify the environment before recording:

```sh
"$B/python" -I -c 'from importlib.metadata import version; print({p: version(p) for p in ("erebus-cli", "erebus-sdk", "erebus-mcp-server")})'
"$B/erebus-payment" --version
command -v anvil cast node jq
df -h .
```

All three packages must report `0.3.0.dev2026101307`.
Payment must report shielded mode and `automatic_rebroadcast: false`.
Keep at least 5 GiB free. Stop if that guard fails.

The local workflow needs the downloaded macOS CI registry, signed pool manifests,
Foundry, Node dependencies, compiled mock contracts and six already-public deployment transactions.
Those inputs are present on this Mac; these absolute paths are not cloud-session instructions.
The harness consumes signed deployment bytes, not a new deployment key.
It replays them only on local Anvil and closes its services when the run finishes.

## Dry Run The Operating Workflow

Run this once before recording. Each attempt needs a new work directory.
The full run can take several minutes, especially with cold artifact downloads.
The edited three-minute video is not a claim about end-to-end execution time.
This drives installed buyer and seller MCP servers, not a simulated conversation or an LLM.
It includes policy-bound funding, encrypted negotiation, settlement, response loss,
buyer recovery, seller restart, resource retrieval and scoped auditor verification.
The explicit demo mode omits extra reservation, backup, endpoint and deadline scenarios.
Its report cannot qualify a release. The full native qualification mode remains unchanged.

The harness calls these real tools; these names are useful during narration:

| Step | Product entry point |
|---|---|
| Fund | Buyer `wrap_wallet`, `approve_wallet`, `deposit_wallet` |
| Agree | Buyer and seller `negotiate_deal` |
| Check | Buyer `check_settlement_funding` |
| Pay | Buyer `settle_deal` |
| Recover | Buyer `recover_deal` |
| Retrieve | Buyer `retrieve_service_access` |
| Disclose | `erebus-shielded-disclosure` JSON methods `export` and `verify_payment` |

```sh
export WORK="/private/tmp/erebus-demo-wmon-$(date +%s)"
mkdir -m 700 "$WORK"
"$B/python" -I "$ROOT/scripts/rehearse-metropolis-workflow.py" \
  --phase run --registry "$REG" --version "$V" \
  --authorized-pins "$PINS" --work "$WORK" --pools wmon --demo-main-only \
  --deployment-journal "$JOURNAL" \
  --prepublication-authorization "$ROOT/packaging/metropolis/testnet-release.json" \
  > "$WORK/workflow.json"
jq -e '.status == "verified" and .demo_only == true and .publication_evidence == false and .claims.live_payment == false' "$WORK/workflow.json"
```

The final check must print `true` and exit zero.
This qualification path uses prepublication pins with published binaries; it is still a local-chain demonstration.
It does not publish anything, send to Monad or change the real buyer/seller/sponsor wallets.
Do not show full private files from the work directory.

Display this restricted result view:

```sh
jq '.pools.wmon.main as $m | {
  status, version, scope: .claims.note,
  buyer_tools: $m.buyer_tools,
  seller_tools: $m.seller_tools,
  payment: {
    response_lost: $m.deal.dropped_broadcast_response,
    buyer_restarted: $m.deal.buyer_restarted_before_recovery,
    verified_by_recovery: $m.deal.payment_verified_by_recovery,
    broadcasts: $m.deal.settlement_broadcasts
  },
  delivery: $m.deal.delivery,
  auditor: $m.deal.auditor
}' "$WORK/workflow.json"
```

Expected: `status: verified`, payment recovery true, one settlement broadcast,
resource verification true, and auditor agreement/payment verification true.
Auditor delivery verification is false: payment verification is not proof of delivery.
This workflow submits directly; do not narrate it as a sponsored payment.

## Dry Run The Monad Segment

This command is read-only and needs no wallet keys:

```sh
"$B/python" -I "$ROOT/scripts/metropolis-demo-readonly.py" --bin "$B" \
  --json > /private/tmp/erebus-demo-monad-1307.json
jq '.sections[] | {label, title, data}' /private/tmp/erebus-demo-monad-1307.json
```

Verify `passed: true`, chain ID `10143`, both runtime pin matches true,
and historical receipt status `0x1` at block `69939428`.
`cast` is needed for runtime hash comparison; without it, pin matches are null, not verified.
If RPC reads fail or any pin differs, stop and investigate before recording.
Do not substitute old JSON while narrating a current chain read.

Historical transaction:
`0xe01666d8c4e16efe8195da8047674c71a325ee20ff250f02f4fdf570a04934fa`.
Explorer: https://testnet.monadvision.com/tx/0xe01666d8c4e16efe8195da8047674c71a325ee20ff250f02f4fdf570a04934fa

## Three-Minute Recording

### 0:00-0:20: Installed Product

Run the package-version and payment-version commands above.
Say: "Erebus gives agents encrypted negotiation, shielded settlement and recoverable paid access.
This is the published Monad testnet release, exposed through CLI, SDK and MCP."

### 0:20-1:40: Actual Two-Agent Operation

Start screen recording before launching a new workflow with a fresh `$WORK` directory.
Run the operating-workflow command and restricted result view above.
Keep an on-screen label: `LOCAL ANVIL - MOCK ASSETS - ACTUAL INSTALLED MCP PROCESSES`.
Edit out waiting periods; do not replace failed results with successful output from another run.
If you show an earlier dry run instead, label that footage `RECORDED LOCAL RUN`.
Do not use a cached JSON report as your only operating demonstration.

Say: "The buyer and seller negotiate through separate MCP servers. The buyer funds a shielded wallet.
A local proof binds settlement to the authorized deal and is checked by the pool contract.
I deliberately drop the settlement response and restart the buyer.
Recovery verifies the payment with one broadcast.
After the seller restarts, the buyer retrieves the resource and verifies its hash.
A scoped grant lets an auditor verify this payment without receiving the full private transcript."

Point to the actual fields for recovery, broadcast count, resource hash and auditor verdict.
Do not claim an autonomous LLM chose the deal: this harness drives the agent-facing tools.

### 1:40-2:30: Monad Integration

Run the read-only command again during the recording.
Display only the network, deployed-pool and historical-payment sections:

```sh
"$B/python" -I "$ROOT/scripts/metropolis-demo-readonly.py" --bin "$B" --json \
  | jq '{sections: [.sections[] | select(.title != "installed commands")], limits}'
```

Say: "These are current reads from Monad testnet. Both deployed pools match their pinned code.
This receipt is a historical shielded WMON payment, not a new payment from the local demonstration.
Its gas sender is public. The receipt does not disclose the amount or buyer/seller payment keys."
Show the explorer briefly if it loads; the CLI receipt is sufficient if the explorer is unavailable.

### 2:30-2:55: Scope And Close

Say: "The public release passed native macOS and Linux qualification.
This is an experimental, single-operator, unaudited testnet system.
Funding, withdrawals and the outer gas sender remain visible.
Fresh-wallet cold-history completion and the full new live sponsored workflow remain open."

Stop before three minutes. Upload to a public video host and test the link while signed out.
Use this new Metropolis video, not the older Starknet walkthrough, in the submission portal.

## Recording Readiness

Preflight verified on this Mac on 2026-10-14:

- Three installed packages report 1307; downloaded wheels match the published manifest.
- Fresh demo-only core run: `/private/tmp/erebus-demo-wmon-core-20261014/workflow.json`, status `verified`.
- Four operator broadcasts occurred on local Anvil: three funding operations and one settlement.
- Lost-response recovery, buyer restart, seller restart, resource hashing and scoped auditor checks passed.
- Live read-only segment: `/private/tmp/erebus-demo-monad-1307.json`; network, both pool pins and historical receipt passed.
- Demo/native-mode/publication regression checks: 53 passed. Demo-only reports are rejected by the publisher.

The earlier full dry run was interrupted during the extra deadline recovery scenario;
it is not claimed as a completed full qualification run. Its files remain at
`/private/tmp/erebus-demo-wmon-preflight-20261014-r2`.
The native full-release qualification already passed separately on both platforms.

You can record the local operating workflow plus historical Monad integration when both dry runs above pass.
The install and native qualification gates have passed, but that does not remove the dry-run check.
A new fresh-wallet live-settlement video is not ready until paired history and live execution succeed.
Independent ceremony, audit, outside acceptance and deployed leakage review remain open.
Do not delay an accurately labelled video while waiting for those external gates.
