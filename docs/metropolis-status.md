# Metropolis Status

Status date: 2026-10-13. Branch: `metropolis`.

This document consolidates the historical Metropolis milestone baselines, progress notes, decision
records, the x402 record, the demo plan, and the submission gates into one status source.
[metropolis-roadmap.md](metropolis-roadmap.md) owns milestone scope and sequencing;
[metropolis-agreement.md](metropolis-agreement.md) stays normative for agreement encoding;
[friction.md](friction.md) stays the separate friction log.

## Current status

### Published release 1307 (2026-10-13)

This entry supersedes the candidate and release-state summaries below.
[Version 0.3.0.dev2026101307](https://github.com/PoulavBhowmick03/erebus-metropolis/releases/tag/v0.3.0.dev2026101307)
is published from clean commit `674e5a77d3cfe7141f32c1221a1335f802f7b34b`.
[Native qualification 38431534413](https://github.com/PoulavBhowmick03/Erebus/actions/runs/38431534413)
passed on macOS arm64 and Linux x86_64, including platform comparison.
Both platforms passed installed onboarding, both-pool HTTPS proving, asset/metered/x402 rehearsals,
the generated two-agent workflow and the installed-relayer gate.
Installed CLI and MCP delegation have separate checks; the relayer reports contain no coverage gaps.
[Public-install verification 38443540443](https://github.com/PoulavBhowmick03/Erebus/actions/runs/38443540443)
also passed on both platforms from fresh environments.

These results qualify installation and local-chain operation, not live Monad settlement or independent acceptance.
Fresh live wallets are funded, but cold-history recovery still blocks their first private operation.
Quicknode passed 1,000 deployment-era header reads without errors at the probe's 40-RPS setting.
Dwellir returned chain 10143 and deployment-era pool code: 100/100 reads passed at 10 RPS;
36/100 reads were rate-limited at 40 RPS.
Per-endpoint scan-rate controls are being prepared separately and are not in the published release.
OpenCode's segmented-cache candidate is not integrated. Live asset/relayer loops, ceremony, audit and outside acceptance remain open.

### Shipping candidate 1307 (2026-10-13)

This entry supersedes older qualification summaries below. Local rebuilt version
`0.3.0.dev2026101307` passed fresh installation, both-pool real HTTPS proving and
the full installed-relayer adapter: eight tests, no coverage gaps. The new HTTP
body handling returned 413 in the unchanged oversized-request test. Installed
payment delegation recorded buyer sends 0, sponsor sends 1 and verified recovery.
The installed MCP sponsor/recovery tools passed the same funded failure matrix,
with their own named test and installed-package import checks.

The [bound report](metropolis-evidence/installed-relayer-1307-20261013.json) names
release manifest `af8ecc13fa4b937ccc9f6aaffebc257c25b7308ca1cbb4ad98e4e93d610e3a34`.
These are dirty-tree macOS team rehearsals with local test keys and synthetic
proving witnesses, not native CI or live Monad qualification. The authorization
names `1307`, conditional on every native publication gate passing.

Next: commit/push the staged candidate, run native macOS/Linux qualification,
publish only after all gates pass, then verify the public install and live loops.
Native MCP qualification, cold-history measurements and live asset/relayer loops remain
open. The currently published version remains `0.3.0.dev2026101202`.

### Integrated rehearsal qualification (2026-10-13)

This summary supersedes the readiness wording in the historical entries below.
The published release remains `0.3.0.dev2026101202`; the new relayer/wallet candidate
is not published. OpenCode handed off dirty source identity
`f6ec6bcc27f39bc39155bf439301a80ec6e0e4361c5083c12fbe6f6db4efb6dd`
based on `482d912`. Claude built local version `0.3.0.dev2026101305` with 16 binaries.
Its release manifest SHA-256 is
`96088431211dd1236a1d5255decd29354ad3ecb12bbb26a2d903f8f6b5f2d374`.

Claude's recorded checks passed real HTTPS synthetic proving for both deployed pools,
two local asset rehearsals, metered access and x402. Installed onboarding refused the
candidate because its binaries do not match the existing published authorization.
That refusal is correct but does not qualify onboarding for the new candidate.
Installed relayer authentication, malformed-request and restart checks passed without
sending; submit/status/receipt recovery remains untested through the installed binary.
A loopback-only Anvil chain-10143 fixture is needed without weakening the relayer's
chain restriction. Oversized requests currently close the connection rather than
returning HTTP 413; this is an observed diagnostic limitation.

Evidence: `.local/metropolis/candidate-20261010/claude-qualification-f6ec6bcc.md`.
These are recorded team-run snapshot results, not independently rerun here, native CI,
live settlement, a release qualification or an audit. The separate paired-recovery
masking investigation found bounded delayed error reporting and no wallet mutation
without complete paired evidence; its three local reproducers passed.

Next: close installed onboarding and funded relayer-binary gates, qualify a clean
authorized revision natively, then execute the approved installed live asset loops.
Cold-history catch-up remains a separate blocker. Independent ceremony, audit and
outside acceptance remain open. Preserve this snapshot while Claude qualifies it;
storage improvements require a separate candidate.

Installed-to-live preparation is recorded in
`.local/metropolis/candidate-20261010/codex-installed-live-execution-handoff-20261013.md`.
The sync reply, asset-unit and funding-intent helper tests passed as three Node
test files with no RPC or keys. This preparation does not close a live gate.

The [leakage review](metropolis-leakage-review.md) covers the candidate's native,
SDK/MCP, relay and disclosure surfaces. Focused Python groups passed 66 and 60 tests;
relay unit tests passed 20, and the observer and Debug redaction tests each passed one.
A mocked native error was echoed by onboarding; Claude owns this defensive hardening.
Wallet amounts/nullifiers are agent-visible, not public-transfer plaintext.
Live observer evidence, gateway/model telemetry review and independent audit remain open.

The [integrated release-readiness review](metropolis-release-readiness.md) is complete.
Its decision is not ready: installed workflow/report gates and integrated relayer inventory
are missing, and ongoing onboarding changes need a new qualified identity.
Focused registry/publication/readiness tests passed 57; the inventory tripwire failed once.
The current authorization covers only the already published version. No new release action was taken.

Integration subsequently resolved the missing relayer inventory and added the new
publication report guards. Focused onboarding/publication/registry tests passed 126.
The loopback chain-10143 fixture was integrated with hashes matching OpenCode's handoff.
Five funded tests passed against Claude's installed `0.3.0.dev2026101305` relayer in 91.73 seconds.
Installed executable SHA-256:
`84cc7e6bae232f4c5a852b98333873f7946a2e267490627b25f0da5001c80168`.
Its package release manifest is `96088431211dd1236a1d5255decd29354ad3ecb12bbb26a2d903f8f6b5f2d374`.
The fixture refusal test passed separately; Rust format checking passed.
Buyer failure coverage here is transport-level, not complete installed payment-CLI delegation.

Subsequent installed payment-CLI delegation passed three times against local package
`0.3.0.dev2026101306`, including timeout after forwarding, durable delegation,
missing auth, restart, unavailable relay, no direct fallback and finalized
observation recovery. Every run recorded buyer sends 0 and sponsor sends 1.
Release manifest: `a479dc79c406008bdfd88c0140ff4bcc481b7568511b41295f88206a2bb6b773`.
The [bound report](metropolis-evidence/installed-delegation-20261013.json) came
from the second full adapter run with a fresh installation outside the checkout.
Its overall gate remains false: the oversized-body test received a transport reset
instead of 413. No assertion was weakened. This is loopback Anvil CLI evidence,
not native CI, MCP sponsorship or live Monad evidence.

Native workflow/report wiring, new onboarding qualification and live execution remain open.
The 5 GiB disk guard was crossed as the run completed; further builds are held.
No files were deleted, commits created or live transactions broadcast.

The generated-agent harness still needs six deployment replay records. A new
`circuits/m5/scripts/export-testnet-replay.mjs` reconstructs those already-public
signed transactions from paired read-only RPCs, using the pinned deployment record.
It checks chain 10143, canonical successful receipts, recovered sender/creation
addresses, exact transaction hashes, runtime code and finality/anchor rechecks.
Fourteen offline tests passed. A real paired read-only export verified all six
creation transactions and wrote owner-only local replay files. No key was read,
new signature created or transaction broadcast. Evidence manifest SHA-256:
`244c1f2662e535568fe3a9e43a2dfb9bbeec761f96af1fee2e4f2cdaae926250`.
The offline suite is included in M5 CI; native qualification still needs to invoke
the exporter and pass its output as `--deployment-journal` to the workflow harness.

Claude's installed two-pool workflow report was read back from
`.local/metropolis/candidate-20261010/workflow-af7e69d8/report.json`.
It records candidate `af7e69d864677f12b02b16d875b4d0a7bbbdafaac571a2d17fc24454c7ab9a53`,
local version `0.3.0.dev2026101306` and release manifest
`a479dc79c406008bdfd88c0140ff4bcc481b7568511b41295f88206a2bb6b773`.
Both pools record payment recovery, one settlement broadcast, resource-hash verification
and auditor payment verification. Total operator broadcasts: 12. These are local mocks
with rehearsal pins, not canonical asset behavior or publication evidence.

Hosted native CI now has a generated-agent workflow step using public replay inputs
and exact-version prepublication authorization. Platform comparison requires the feature
reports for hosted relayer packages. Seventy-seven focused workflow/publication/registry
checks passed; workflow YAML parses and diff checking passed. No native run was dispatched.
The installed-relayer report adapter and its CI invocation remain open before qualification.

An integrated default Python workspace run (`uv run --frozen pytest -q`) passed
830 tests with 13 skips in 67.68 seconds. This ran on the shared working tree;
it is not frozen-candidate qualification, funded native CI or an installed live run.
The immutable-resource access service deliberately permits late issuance without
voiding payment (`access.rs:260,732`). The client checks that the seller's late flag
matches the signed deadline, but its cached receipt drops issuance time/late metadata.
The strict SDK consequently does not expose that detail to the agent. This is a
remaining diagnostics/ergonomics task, not a demonstrated false delivery claim.
The resource hash can verify while `delivery_verified` remains false.

### Authorized artifact release and qualification (2026-10-12)

The owner authorized local publisher creation, manifest signing, artifact hosting,
candidate commit/push and package publication after all native qualification gates pass.
The public release policy is `packaging/metropolis/testnet-release.json`.
Publisher: `0xcd71b49f350f536ee4c4a27c8d0104be91f684fe`.
Both detached signatures passed artifact preflight against their deployed pool domains.
The persistent publisher key stays owner-only in the ignored operator directory.
It is not a fixture key, but it remains a team-controlled identity.

The artifact prerelease `artifacts-268bf1abe34d` now contains 17 public assets:
nine proving files, three verification keys, checksums, two manifests and two signatures.
Both pool verification checks and full artifact/key fetches passed over real HTTPS,
with unchanged manifest pins and hash/size checks. Local fetches used curl's certificate
validation and bounded connection fallback after Python stalled on a CDN connection.
These are team-operated download checks, not installed proving or native qualification.
Evidence: `.local/metropolis/candidate-20261010/https-fetch-{wmon,usdc}-20261012.json`.

The integrated candidate was committed and pushed as
`5ba05ee2999aecb390c2d8276f084af38dc4dda3`; the merge is closed and `main` is untouched.
Python verification passed: 670 tests, 38 skipped. Staged secret scanning passed.
Native hosted-input qualification `38221534481` failed: macOS Bash 3.2 lacks
`mapfile`, so its registry build never ran. Claude reported the native Linux leg
passed its hosted proving and installed rehearsal gates; that does not qualify both platforms.
The four packaging-only fixes were verified (267 scripts tests passed, 9 skipped),
committed and pushed as `07e6949c9a6593cc8c051ddbd096ea64c82e7ee1`.
Relayer and historical M4 changes were excluded. Hosted re-qualification is running as
[run 38227563253](https://github.com/PoulavBhowmick03/Erebus/actions/runs/38227563253),
with the same publisher, pool manifests, version and `publish=false`.
No private package publication or complete native qualification success is claimed yet.

The broader CI run `38221213877` failed its M4 prototype because its historical
note hash was compared against M5 v2's updated Rust payment-note vector.
The local fix pins M4 to the unchanged Rust vector from `94aa059` under its own
fixtures directory. Its SHA-256 is
`ea554a298ccd1a1e7861e1f3c97b1a0d2fe21016b6687f749e5ccd97167c49a4`.
`npm --prefix circuits/m4 run prototype` passed proof, local EVM settlement,
replay and mutation checks. Current circuits, keys, deployed verifiers and
packaged input-tree hash are unchanged. This fix is uncommitted; the CI result
remains failed until a corrected revision runs successfully.
The planned private package version is `0.3.0.dev2026101202`.
Qualification uses `publish=false`; authorization does not permit bypassing a failed gate.
This remains test-only, single-operator and unaudited. Independent acceptance is still open.

### Shielded relayer admission boundary (2026-10-12, uncommitted)

`sdk/shielded/src/relay_admission.rs` verifies externally supplied transfer calldata
against a hash-pinned zkey's verification key, without witnesses or spending material.
It reuses prepared deployment/identity checks, rejects noncanonical field encodings,
checks curve/subgroup membership, handles Solidity's G2 coordinate order and enforces
the supplied paired-observation timestamp against expiry.
All 95 shielded library tests passed. The ignored external-proof test accepted a
retained local transfer proof and rejected all eleven changed public inputs, damaged
points, wrong/truncated keys, noncanonical fields, changed selector and exact expiry.
Release all-targets clippy passed. These tests use local known-entropy artifacts.

Paired admission authenticates the configured creation anchor and all five runtime pins
at creation and the shared canonical head. It checks the pool asset/version, verifies
expiry against that head's timestamp, and simulates exact transfer calldata with the
configured sponsor, gas limit and zero native value through both providers.
The pinned pool's simulation checks roots, consumption, output reuse and capacity.
Creation, head and finalized anchors are rechecked, and regressed finality is refused.
The two ignored admission tests passed with known-entropy local artifacts. Controlled
RPC tests cover honest lag, conflicting headers, wrong runtimes/assets, reverted or
malformed simulation results, changed anchors, regressed/missing finality and wrong chains.
Invalid proofs are refused before RPC reads. No RPC write method is invoked.
These providers simulate evidence; this is not funded contract or Monad admission evidence.

The separate funded Anvil test now executes admission against the actual pool and
verifiers. A valid proof is admitted without changing the sponsor nonce. A valid
unknown-root proof is refused. The test submits one payment itself from a sponsor
account; admission then refuses the consumed deal, a fresh deal spending that input,
and a second unspent input reusing the first payment's change commitment.
The pool's exact revert reasons are checked, both providers use pinned simulation,
and refusals never advance the sponsor nonce. All three admission tests passed
(17.18 seconds, release profile); all-targets clippy passed.
The fixture uses existing known-entropy artifacts, creates no setup keys, and accepts
only loopback Anvil chain 31337. This is funded local evidence, not a relayer service,
Monad rehearsal, audit or independent acceptance. CI now includes the ignored suite;
that new CI step has not yet run remotely.

The buyer delegation fence is implemented in the working tree. It binds one
operator-pinned relay identity to the digest of retained prepared calldata, under
the coordinator's identity lock. The record is durable before returning to the
caller. Repeated identical delegation is idempotent; another relay conflicts.
Existing direct signing plans cannot delegate, and delegated operations cannot
invoke the direct signer. The payment driver observes delegated operations without
direct submission. Finalized evidence can still reconcile the payment.

Coordinator release lifecycle tests passed (29 passed, one exhaustive test ignored), including
restart, expiry, direct-signing exclusion and injected delegation-write failures.
The shielded release library rerun passed 97 tests; coordinator and shielded clippy passed.
These are local tests, not service or Monad sponsorship evidence. Delegation is an
optional record field: unchanged records remain readable, while older strict readers
reject delegated records instead of silently dropping the fence.

No relayer service, sponsored service send or live relayed payment exists yet.
The request codec accepts only a version and canonical transfer calldata, within a
2048-byte JSON limit. Unknown or duplicate fields are refused. Metadata is derived
against operator configuration; the service operation identity binds chain, pool
and deal nullifier, not proof bytes or a client-selected identifier. A different
proof for the same payment has the same identity and must conflict with changed
retained calldata. Both codec tests passed; decoding is not proof admission.
The concurrent delegation test also passed: exactly one relay wins and the loser
conflicts. HTTP authentication, integrated service accounting and sponsored submission
are still unimplemented.
The sponsor journal now retains the canonical public request, validates decoded
signed bytes against sender/chain/pool/value/gas/fee/calldata, persists a plan before
the signer runs, and fences initial broadcast before returning outbound bytes.
Changed proof bytes under the same identity conflict. Corrupt or deleted records
with retained markers fail closed. Four local tests cover restart, competing callers,
wrong signed targets and faults at each signing/broadcast write boundary.
The complete release library rerun passed 101 tests, and production library/binary
clippy passed. All-targets clippy found `too_many_arguments` in OpenCode's new
`tests/relay_adversarial.rs:344`; that independent test file was not edited here.
No nonce integration, HTTP service, fee replacement, finalized service status or
live sponsorship is implied by these journal tests.
Read-only admission is not a reservation, payment or authorization to submit.
Funding checks, immediate pre-sign re-admission and durable service/buyer fences remain
mandatory before allocating sponsored gas.
The pinned native release run tests packaging commit `07e6949`, not this new working-tree module.
Run `38227563253` completed successfully: native Linux, native macOS and platform
comparison passed; publication was skipped as requested. This confirms workflow
completion, not uploaded package hashes or a fresh public install. Claude owns those
remaining release checks under the recorded authorization.

### USDC rehearsal amounts (2026-10-12)

The operator preparer previously reused WMON's 18-decimal raw amounts for USDC.
A read-only `decimals()` call to the configured Monad testnet USDC returned `6`.
Preparation now uses asset-specific base units for deposits, prices and negotiation bounds.
WMON amounts are unchanged. USDC uses a `1500`-unit deposit and `700`-unit seller quote.
The helper tests cover both assets, negotiation bounds and unsupported names.
`npm --prefix circuits/m5 run test:live-amounts` and the preparer's syntax check passed.
The package candidate hash remains unchanged; no circuit, setup key or binary changed.
A read-only USDC balance call for the retained buyer returned `0`.
No USDC participant directory or payment was created by this change.

### r7 local package qualification (2026-10-12)

Claude reported all installed macOS gates passed for `0.3.0.dev2026101201`:
onboarding, both proving domains, disclosure configuration, assets twice, metered access and x402.
The source candidate remained `e4e4f8323ba40e34ac4701d04d52d55f1ca5182328d9b2bf81a75fcc8f495130`.
Codex independently checked the release checksum and all 22 files in `r7-INDEX.json`; every checksum matched.
Release SHA-256: `02b20b0a2b5d4d07853848b12e89648992752c684318087485d33d9089214b08`.

The pre-run harness checksum capture failed because of a quoting error.
Claude used later checksums and modification times to assess stability, not a successful pre-run hash comparison.
Funded rehearsals used Anvil; offline witnesses were synthetic; artifact fixtures used local transport.
The candidate remains dirty, test-only, single-operator, unaudited and release-ineligible.
No native CI, real HTTPS hosting, private package publication or outside acceptance is established by r7.
The merge was closed in `5ba05ee`; packaging fixes were committed in `07e6949`.
New relayer work remains uncommitted and is excluded from that frozen release.

### Retained wallet recovery (2026-10-12, read-only continuation)

Both retained wallets ran bounded syncs with the fixed withdrawal binary.
Binary SHA-256: `91eac0cf76032daa46061c8d8150735b7dd41c4e3a9bcae01e93b6cb277bd924`.
The latest seller prefixes are `69931652` and `69919652` after a further bounded continuation.
The latest buyer prefixes are `69950934` and `69944934`.
Three parallel steps advanced primary caches faster than peer caches.
A subsequent serial seller step advanced both prefixes by another `1000` blocks.
Completed scans returned pending. None requested signing or a transaction.
Their owner-only reports remain in each participant's timestamped sync directory.
Incomplete history is not evidence that a note is unspent.
Withdrawal, unwrap, repeat spending and the USDC live workflow remain open.
The sync reply-channel and fail-closed parsing tests passed; `git diff --check` passed.

### Withdrawal completion attribution (2026-10-12, source candidate)

OpenCode identified a withdrawal accounting defect in `reconcile_operations`.
The old path completed a withdrawal from note consumption alone.
In an older-attempt race, it credited a rejected replacement at block `0`.
The stronger regression reproduced this result against the old binary.

Withdrawal completion now requires its own successful finalized receipt.
Wallet consumption must match that attempt's transaction hash, block number and block hash.
Receipt reconciliation records the actual winner and clears only the matching reservation.
Restart recovery permits an already cleared reservation when the same receipt still matches.
Generic wallet reconciliation no longer completes withdrawals.

The stale-backup test originally reused the completed original operation.
Its `Completed` reply represented an idempotent lookup, not a second execution.
The corrected test uses a fresh operation journal with the stale wallet and index.
That case passed before the fix: the second attempt reverted and the payout remained unchanged.
The replacement-attribution regression failed before the fix and passed afterward.

Executed checks: 91 release library tests, all non-ignored targets,
all-target release Clippy, and both installed wallet tests passed.
The installed tests took 176.91 seconds and covered withdrawal and native unwrap.
All ten ignored wallet fault tests passed in 596.89 seconds.
Both fresh ERC-20 and wrapped-native workflows passed in 255.11 seconds.

Source identity: `e4e4f8323ba40e34ac4701d04d52d55f1ca5182328d9b2bf81a75fcc8f495130`.
Operation source SHA-256: `a1f8568237e2f61d7bddd1db82461976e3f491b3151f887c13155cf6220dcaa9`.
The candidate remains dirty, uncommitted, test-only, single-operator and unaudited.
Claude's r6 evidence remains valid only for its earlier source identity.
Circuits, setup keys, manifests and deployment records did not change.
No live transaction was sent. Live wallet catch-up, withdrawal and unwrap remain open.

### Authorized shielded pool deployment (2026-10-11, Monad testnet)

The owner authorized deployment and funded the retained dedicated gas address with
15 testnet MON. Six creation transactions finalized on chain 10143: Poseidon,
deposit/transfer/withdraw verifiers, and asset-specific WMON and USDC pools.
The executor persisted signed bytes and attempt records before sending each transaction.
The separate read-only checker verified all creations through the Monad public endpoint
and Ankr, including transaction input, canonical finalized receipts, exact runtimes,
asset runtime pins and constructor getters. Provider agreement is not a consensus proof.
Reviewed prepared-plan SHA-256:
`c7dffb78163c52b1f8ad46b143960afe7249a2e4cc98e46babdbe0f5497ae34a`.

| Pool | Address | Creation transaction | Block |
|---|---|---|---|
| WMON | `0x8868f35140f3602C80E2C76132740B6f0c0b409D` | `0x724d0d280b1fa950e11d60fcc0e10982222a6b562eea6d2b529fbc60f374a55f` | 69902653 |
| USDC | `0x863f540471C0aD33c68D1218f31E8C9fe9cD50FB` | `0xb5fad2d6723d2b7556b6b1c16b9f036be37caa0794020563b828cb46639890e7` | 69902683 |

Total receipt gas: 11,553,585. Gas cost: 1.17846567 testnet MON.
The remaining deployer balance was 13.82153433 MON after these six transactions.
Local execution evidence and the private deployment journal are retained under
`.local/metropolis/candidate-20261010/`; signed bytes and keys must stay out of public reports.
The checker output's `live_transactions_sent: 0` describes its read-only verification,
not the preceding executor, which sent six transactions.

This is test-only, single-operator setup and unaudited candidate code from the dirty tree.
One WMON deposit and one private payment subsequently finalized, as recorded below.
USDC payments and withdrawals remain open. Resource retrieval after seller restart and
recipient-only payment auditing subsequently passed, as recorded below.
Runtime identity does not qualify token behavior or remove USDC issuer/admin powers.
Deployment-bound manifest generation and hosting, installed workflow evidence, native
release qualification, independent audit and outside acceptance remain separate gates.

The existing bundle builder subsequently generated two local pool-bound manifests,
rebuilding source artifacts and verifying all three retained zkeys and verification keys.
No setup entropy or keys changed. WMON manifest SHA-256:
`46d8bda2c69badff64046976a727c25a0fa05bd443484983108d3f1d9f5beb77`.
USDC manifest SHA-256:
`d52aa67cc1c5904e3b5be3a6157af5f2af3ecedec27eec85414cb7edf7257347`.
Bundles are retained as `live-bundle-wmon` and `live-bundle-usdc` in the local evidence
directory. URLs remain explicit not-hosted placeholders; these are not public install inputs.
Public read-only deployment evidence is in
[monad-testnet-shielded-v2.json](../contracts/evm/deployments/monad-testnet-shielded-v2.json).

### First finalized private WMON payment (2026-10-11, team-run)

The retained suite-2 agreement settled through the deployed WMON pool. Product observation
returned `stage: Finalized`, `payment_verified: true`, `broadcast_attempts: 1` and
`submitted_this_call: false`. Durable coordinator accounting is `Committed`.

| Transition | Transaction | Result |
|---|---|---|
| Wrap 0.0015 testnet MON | `0x794d384ce3e2fce48589f63ccf8720619c7766fb63652964c37fb66d542597c4` | Finalized; exact WMON Deposit event |
| Approve exact pool allowance | `0x9bf702fd1e3efc071bd3c3f725624929fb7e5ee109b68fa446ed329024069f7b` | Completed; signer claim released |
| Proving WMON deposit | `0xd6d6bc404990208e60bd56919684fb29ad70439100fb348830e5f54c2d8c7c02` | Completed; confirmed note, signer claim released |
| Private payment | `0xe01666d8c4e16efe8195da8047674c71a325ee20ff250f02f4fdf570a04934fa` | Receipt success; finalized product verification; one broadcast attempt |

Deal commitment: `2f6a750128ed71d96ae160eeff816d3e3aa8240bfa20115c82f9c5d339691419`.
Deal nullifier: `289d55099a41a9ee1d8811c7ae32c8c82b97e854cab125e34f2a81cfe141d6e2`.
The authorized payment is 0.0007 WMON; local proof preparation reported 2,874 ms.
That amount is disclosed here by the team, not read from the transfer receipt.
The receipt contains pool note/deal events and no ordinary ERC-20 Transfer event.
The gas-payer address, pool, funding/deposit activity, timing and note/deal identifiers
remain public; a small test pool does not establish meaningful anonymity.

Negotiation used installed candidate r3 commands; settlement used hash-frozen source-built
commands with the read-retry fix. Public-history caches were warmed from independently
built paired wallet histories, then replayed and checked by product commands. No keys,
note openings or participant accounting were copied into verifier caches. This is warm-cache,
team-run testnet evidence, not a released installed-only or outside acceptance workflow.
Setup remains single-operator, test-only and unaudited.

The first settle stopped before preparation because the local manifest was mode 0644.
After tightening it to 0600 without changing its hash, durable state and a product observe
reply confirmed no preparation, signing or broadcast attempt. The same agreement then
resumed and submitted once. No settle call followed that broadcast attempt.

Access service health reported `payment_submission: false`, but retrieval did not succeed.
A retained retry reported `access service unavailable; retain state and retry without another
payment`. The signed delivery deadline subsequently passed. The service is stopped and
its state retained; neither the agreement nor its deadlines were changed, and no second
payment was requested. The original grant is retained; a fresh recipient-bound grant for
the already-settled deal was exported for the auditor check. That command ran with buyer,
seller and onboarding paths renamed away, then restored all three paths. It failed closed
with `payment not independently verified; retain caches and retry`, after advancing both
public prefixes to 69940568. No transaction was sent. Audit success is not claimed; the
recipient-only check still needs further history catch-up and a verified result. Evidence
is retained under `live-wmon-operators` in the local evidence directory.

### Live shielded auditor completion (2026-10-11, source-built team run)

The updated auditor verified the retained WMON deal with only its encrypted grant,
auditor key, authenticated public configuration and public history caches.
Buyer, seller and onboarding paths were withheld during the command and restored afterward.
Result: `agreement_verified: true`, `payment_verified: true`, `delivery_verified: false`.
No transaction was submitted and no agreement or payment expiry was changed.
This is a separate process operated by the team, not independent outside acceptance or a security audit.

The earlier observer tried to scan roughly 96,000 unrelated later blocks despite an indexed payment.
Positive certification now accepts a complete shared prefix containing the authorized payment,
only after both providers place that canonical prefix below their explicit finalized tips.
Canonical payment events, consumed state, pool root/leaf count and runtime pins remain required.
Missing payments and wallet/coordinator reconciliation keep the full-history path.
The prefix is not evidence that later notes remain unspent.

The auditor now requires pool, Poseidon and all three verifier runtime pins.
Incomplete history returns exit code 2 with progress rather than a generic verification error.
The client retrieval timeout is 240 seconds with a 300-second request lifetime.
The signed agreement deadlines are unchanged; late issuance remains explicitly marked.

Executed local tests: 90 release library tests passed, including the 31-second HTTP response test
and `cached_finalized_payment_does_not_chase_later_activity`. All-target release Clippy passed.
The cached-payment regression rejects missing events, inconsistent consumed state and wrong runtime code;
an empty cache still returns history pending. These are source-candidate tests, not package qualification.

The final access-seam library rerun passed all 90 tests. The broad funded payment-driver run
passed 26 tests and failed one timeout fixture: its single delayed read was retried before the client timed out.
The fixture now delays four consecutive reads without weakening its assertions.
Its targeted rerun passed in 137.84 seconds with a real client timeout, retained checkpoints,
verified resource recovery and one payment send. The complete 27-test command was not rerun afterward.
The public-bound access HTTP regression passed, and Python disclosure/config seams passed 37 tests.
Final all-target release Clippy and `git diff --check` passed.

Live auditor binary SHA-256: `d1b1fe62788a58a8a8c68a345f2d5f4a6c70582baf6972dd583a3525e7944e0b`.
Evidence: `live-wmon-operators/auditor/verification-audit-1791699692814/result.json`
under the local evidence directory. Original binaries, grants and failed attempts are preserved.

### Live shielded resource retrieval after restart (2026-10-11, source-built team run)

The buyer retrieved the retained paid snapshot through the updated access service.
The service was stopped and restarted, then the buyer repeated retrieval with an empty,
separate cache. Both requests returned the same issuance ID and resource SHA-256.
The service retained exactly one issuance and reported `payment_submission: false`.
No second agreement, permit, payment or transaction was created.

Both buyer results report `resource_verified: true` and `seller_reported_payment_finalized: true`.
They keep `payment_verified: false` and `delivery_verified: false`; the separate auditor supplies payment verification.
The payload is a 308-byte team snapshot, not evidence of an external provider integration.

Issuance occurred after the signed delivery deadline. The persisted issuance timestamp is
1791699766 and the original deadline is 1791681012. The policy permits late retrieval;
this demonstrates recovery, not timely delivery. No deadline or agreement was extended.

Evidence: `verification-retrieve-1791699746737/result.json` and
`verification-retrieve-1791699809442/result.json` in the retained auditor evidence directory.
Service binary SHA-256: `9061f66a13a0737aa6ca2e8d786b2f59cad8a64a2888596d8bd9f489490ac8a1`.
Client binary SHA-256: `3f014ba6abd5a4831502e51a9f16cd2cb179955790a0bc3856dd5263c4b4f81f`.
The service was stopped after the check. These source-built, warm-cache runs do not qualify published packages.

### Deployment lane regression and installed workflow preparation (2026-10-11)

The local deployment-executor interruption suite found a fresh-journal bypass:
an older unmined creation inflated the pending nonce, allowing a new plan to start
at the next nonce. The executor now requires both providers' mined (`latest`) and
pending account nonces to equal the planned nonce before signing and before
persisting a new attempt. Retained attempts remain observation-only. The native
participant-funding helper applies the same guard.

The executor suite passes against local Anvil, including signed-before-attempt
interruption, a lost broadcast response, finalized resume without another send,
corrupt records, conflicting receipts/runtime evidence, and an older pending attempt.
The fresh-journal bypass regression specifically requires the hold error, zero sends,
and no signed or attempt record. Its interruption hook counts only `pending` nonce
reads, so the additional mined-nonce read does not move the simulated interruption.
Funding signature/intent tests also pass; they are not a funded interruption sweep.
No Rust settlement code or deployed contract changed for this fix.

Two separately generated participant gas accounts received 2 testnet MON each.
Installed buyer and seller commands completed encrypted suite-2 negotiation and
the seller published its accepted-deal evidence through the product path.
These are team-run local candidate wheels with a verified warm artifact cache,
not a public-install or independent-operator acceptance run.

The read-only wallet catch-up helper now parses pending/error replies from stderr
and successful replies from stdout, checking that status agrees with the exit code.
Its parsing tests pass. The subsequent buyer sync saved a public prefix through
block 69907386, then failed closed with `paired wallet recovery failed`; the peer
prefix was still absent. No wrap, approval, deposit or private payment was requested.
The retained caches, keys and agreement are unchanged. Diagnosing the paired scan
failure is the next live-workflow task; it is not evidence of a completed private payment.

Offline qualification witnesses are now available for both deployed pool domains.
`qualification-witnesses.mjs` uses fresh synthetic openings and synthetic Merkle histories,
without reading participant keys, contacting RPCs or sending transactions. JS fullProve
and verification passed deposit, transfer and withdrawal for each pinned pool bundle.
The wrong-pool manifest is refused before output creation. These are not live wallet
inputs, authenticated on-chain histories or payment evidence. Claude reports that the
installed r3 hosted fixture proved and verified all three transitions for both domains.
Wrong publisher, changed manifest, foreign-domain public inputs and tampered server
artifacts were refused; a corrupted cached artifact was downloaded again. Reports are
`macos-hosted-proving-wmon.json` and `macos-hosted-proving-usdc.json` in the retained
candidate evidence directory. These are team-run synthetic fixtures, not live payments.
`proved: true`, `https_exercised: false`, `release_recorded: false` and `test_only: true`
remain the exact scope. The r3 packages do not contain the newer RPC read-retry change.

A separate public-history diagnostic reproduced a peer `eth_getBlockByNumber`
request failure during a 1,000-block slice; the primary saved its verified slice.
Both endpoints passed a smaller 100-block scan and historical pool-state calls.
The failure class is known, but the diagnostic did not expose its HTTP status.
The source now permits at most four attempts for explicitly allowlisted read methods
on connection/timeout errors or HTTP 429/502/503/504. Each attempt uses the existing
pacer. Invalid JSON-RPC responses, permanent HTTP errors and evidence conflicts still
fail closed. No broadcast method is retried. The targeted retry matrix and Clippy pass.
Installed r3 binaries do not include this change, and the live catch-up must be rerun
with a verified updated candidate before recovery can be claimed.

The updated source-built wallet subsequently completed one bounded live read-only
sync step: primary prefix 69908386, peer prefix 69903652, exit 2 (`history pending`).
Binary SHA-256: `6b3886eb2412e9d076764d62fce25fc2afb882d6d82f37ee3ffd0c819b8df4d4`.
This confirms saved progress under the new build, not completed wallet recovery or
installed-package qualification. The helper records source-build override use and
binary hashes, accepts a bounded `--max-steps`, and its reply/option tests pass.
The full shielded library suite passes (86 tests); release Clippy for library/tests passes.
The published and locally installed r3 binaries are unchanged. No live deposit or
private payment was sent during this step.

Two further bounded steps also exited 2 with saved progress and no RPC failure:
primary prefix 69910386 (7,734 blocks), peer prefix 69905652 (3,000 blocks).
The same source-wallet hash was recorded for all three steps. Full recovery is
still pending. The packaging identity check records dirty input-tree SHA-256
`94d9eb6ef65909b2893184b56ff3fc5cfd11cb7dec4d32dd4797deaf473746b1`,
191 input files and `release_eligible: false`; this is not a clean release freeze.

After reviewing both installed-domain proving reports, Codex recomputed the same
candidate identity and completed a fourth bounded read-only step: primary prefix
69911386, peer prefix 69906652, exit 2, no RPC error. Both prefixes advanced by
1,000 blocks. Full wallet history remains pending; no wallet mutation or private
payment was requested. The pinned packaging rebuild handoff is retained under
`.local/metropolis/candidate-20261010/codex-packaging-rebuild-handoff-20261011.md`.

OpenCode's retry regression report matches the current RPC source hash
`46c9bf1981fe12ccdb4814e2cae5599ffa2526dc5804d651470c253efd972b3c`.
The release controlled-pair suite passed (6 tests, 127.81 seconds), followed by
the ERC-20/WMON workflows (2 tests, 259.07 seconds). Three new local tests each
passed twice: transient header/log failures recover without lost progress;
exhausted retries retain prefixes and reservations; exhausted reads after a
retained consumption never revive the note. The tests assert exactly four
attempts at exhaustion and one total payment send through recovery. Codex checked
the report, test-source hash and relevant assertions; these runs were performed
by OpenCode, not independently rerun by Codex. Evidence is retained in
`opencode-wallet-retry-20261010.json` under the candidate evidence directory.
This is local Anvil/test-only evidence, not a live payment, audit or release gate.

The next source-built live read-only sync step saved primary prefix 69912386 and
peer prefix 69907652, each another 1,000 blocks, without an RPC error. Exit 2
still reports incomplete verified history. No transaction was requested.

A subsequent ten-step read-only run ended with primary prefix 69921234 and peer
prefix 69915652. All replies reported pending history; the peer did not advance
on two slices and later resumed. Direct numeric-block probes from both providers
matched the retained peer tip. This does not establish complete paired history
or explain the stalled slices. No transaction was requested.

The updated source-built payment, access service, disclosure and access commands
built successfully in release mode (39.83 seconds), with the package input identity
unchanged. Exact binary hashes are retained in `codex-live-build-20261011.md`.
The canonical coordinator accessor confirmed the retained agreement expiry
1791681288 and delivery deadline 1791681012 were still future at inspection.
These builds are not installed-package qualification.

Claude's updated registry build stopped at the disk guard before package output.
Codex measured 4.95 GiB free after the run, below the 5 GiB guard. Further builds
and wallet mutations were held pending restored disk space. The owner then approved
removing only `sdk/rs/target/debug`. `cargo clean --manifest-path sdk/rs/Cargo.toml
--profile dev` completed (424,302 files, 28.9 GiB logical bytes removed), and the
filesystem reported about 24 GiB free. The directory is absent; source-wallet,
payment-command and both pool-bound manifest hashes remain unchanged. No release
binary, proving bundle, retained evidence, key or journal was deleted. Claude's
packaging lane was unblocked, and live read-only catch-up resumed. Payment was still
open at that stage; its subsequent finalized result is recorded above. Updated
installed-package gates remain open.

The live buyer subsequently completed paired wallet catch-up through block 69926520.
One wrap of 0.0015 testnet MON completed from finalized wrapper evidence at block
69926980 (`0x794d384ce3e2fce48589f63ccf8720619c7766fb63652964c37fb66d542597c4`).
One exact WMON approval to the WMON pool was submitted
(`0x9bf702fd1e3efc071bd3c3f725624929fb7e5ee109b68fa446ed329024069f7b`);
the finalized allowance read is 1,500,000,000,000,000 base units. Wallet history
catch-up and approval reconciliation were still pending at that stage. The subsequent
deposit and private transfer results are recorded above. These source-built
commands are hash-frozen for the live run; this is not package qualification.

### Paired shielded snapshot correction (2026-10-10, local gates passed)

Claude's unchanged installed binaries failed the asset rehearsal twice, then passed once.
That pass is diagnostic evidence, not closure of the intermittent failure.
The paired observer and wallet recovery independently sampled tips and compared complete
snapshots. A block boundary or honest provider lag could therefore cause disagreement.

The current source selects a common finalized height below both explicitly reported
finalized tips and a common head below both observed heads. Both providers must return
identical canonical block headers there. Both scans and state reads use those anchors.
Post-read canonical and finality checks remain required; conflicting history/state still
fails closed. Missing finality never becomes unpaid evidence. No signing, reservation,
nonce or broadcast format changed.

RPC regressions accept honest head/finality lag and reject conflicts at either shared
anchor, missing finality, reuse of the same endpoint, and a finalized block changing on
both providers during selection. Finalized wallet replay also refuses to rewind a retained
prefix to an older anchor, so provider lag cannot resurrect an observed spent note.
That guard's regression passes without a network read or cache rewrite.

The initial shared-snapshot change passed all 83 then-existing library tests. The two
snapshot regressions and wallet regression pass separately. After the final wallet guard,
all 12 finality tests and all-targets clippy with `-D warnings` pass again.
Both funded ERC-20/WMON workflows passed in release mode (2 tests, 259.41 seconds) before
the last wallet-regression guard. The slow debug run was interrupted, not counted as a pass.
OpenCode's final rerun reports 85 library tests passing and both funded ERC-20/WMON
workflows passing (2 tests, 254.67 seconds), after the wallet guard. Four controlled
real-CLI regressions pass (162.13 seconds): moving tips, honest provider lag,
same-height conflicting chains, and lagging finality after a retained spend.
The tests check broadcast counts, reservation/consumption state and observation-only
recovery. Genuine conflict remains fail-closed; a lagging anchor cannot revive a spent note.
Codex reviewed these assertions and matched all six reported production/test file hashes
to the current source. These are reviewed agent-run results, not new Codex test executions.
Evidence is retained in `.local/metropolis/candidate-20261010/opencode-paired-regression-20261010.json`.

OpenCode's subsequent tests-only handoff reports six controlled CLI tests passing
(202.23 seconds), including conflict after finalized payment and interrupted paired
recovery across eight pending cycles. One-send accounting and retained consumption survive
both cases. A non-ignored unit regression requires explicit broadcast diagnostics before
a settle retry; absent or null fields cannot stand in for zero attempts.
These remain reported local Anvil/proxy/fork results, not Monad or audit evidence.

Claude's rebuilt macOS arm64 package `0.3.0.dev2026101003` passes fresh installation,
onboarding, cold proving/cache/tamper checks, metered access and x402. Both installed
asset workflows pass twice (278.40 and 265.87 seconds). Codex independently recomputed
the matching package input-tree hash:
`0b93e2a04dbe1610ccf65712a54074fb929a5b47e9774a3a19ed3261b69dd72c`.
The proving manifest remains `268bf1abe34d738859a20d5db8cee96a30fd4dbb4b2e25b8b5b9da7084d59a4a`.
This closes the reproduced paired-snapshot blocker locally, not every possible RPC failure.

The matching Linux Rosetta build stopped at the disk guard; there is no Linux result
for this candidate. Earlier emulated builds are neither current nor native qualification.
Do not remove retained evidence, VM images or another agent's build outputs without approval.
The open merge and dirty candidate remain release-ineligible, unaudited and test-only.
No live transaction, publication, commit or deployment occurred in these runs.

### Funded access example configuration repair (2026-10-10, local only)

OpenCode found that the coordinated access example omitted the five required runtime pins.
The example now reads its own local fixture deployment's pool and dependency runtimes at
the deployment block and supplies all five fields. The service retains its strict checks.
These fixture-derived pins are not independently reviewed live-deployment trust roots.
The startup check now requires a successful health status, not merely an HTTP response.

`cargo check` and clippy with `-D warnings` passed for `coordinated_transfer`.
The pinned-bundle prototype passed with `EREBUS_M5_NATIVE_PROOF=1`,
`EREBUS_M6_COORDINATED=1` and `EREBUS_M8_ACCESS=1`: one payment finalized, the separate
auditor verified agreement/payment without participant state, and access recovery retained
one issuance after restart. Deposit, output recovery, withdrawal and replay checks passed.
This rerun omitted the crash matrix; a duplicate matrix run was stopped after boundaries
0 through 4. OpenCode's separate full-sweep report is not a new Codex verification.
All execution was local Anvil with test-only setup. This configuration repair did not
resolve the intermittent asset failure; the separate shared-snapshot correction and
final installed regressions above address its reproduced cause.

### Shielded deployment preparation and verification (2026-10-10, local only)

`prepare-testnet-deployment.mjs` validates the current circuit and witness source inventory,
stages the explicitly pinned test-only bundle, and compiles the production pool contract.
It prepares CREATE calls for Poseidon, three verifiers, and one or two asset-specific pools.
Predicted addresses and runtime hashes include each pool's constructor immutables.
It does not load keys, call RPC endpoints, sign, or broadcast.

`check-testnet-deployment.mjs` verifies creation receipts, exact transaction input, runtimes,
asset runtime pins and constructor getters through two RPC endpoints. It requires explicit
canonical finalized evidence, pins state reads to one shared block, and rechecks the anchors.
Provider consistency is not a consensus proof or asset qualification.

The checker CLI now requires a separately retained SHA-256 pin of the reviewed prepared
plan before any RPC read, and records it in its output. Byte changes, missing pins and
wrong pins are rejected. This binds verification to the supplied reviewed file; it does
not make an unreviewed plan trustworthy or replace release authorization.
After this change, Codex reran `npm --prefix circuits/m5 run test:deployment` with
the pinned bundle: six local deployments, paired finalized verification, provider-mutation
rejection and plan-pin rejection passed. No live chain was used.

The bundle-backed deployment test passed on local Anvil: six deployments matched the prepared
identities. Read-only verification passed, including honest finalized-height lag. Changed chain,
missing or forged finality, changed code, reverted receipts, changed transaction input,
changed canonical hashes and wrong configuration pins were rejected.
The test uses mock assets only as fixtures; the planner deploys no mock contracts.
Anvil uses a shortened finality lag for this test, not Monad consensus.

No Monad deployment, hosted artifact, updated package qualification or release is claimed.
The prepared output requires deployment-bound artifact manifests before installed use.
Commands and input fields are in [metropolis-operations.md](metropolis-operations.md#shielded-testnet-deployment-preparation).

### Refreshed test-only bundle provenance (2026-10-10, local only)

Claude's package rehearsal used manifest `37fed4d21ea6d68f783a4ccb65879764a9d0487f51397c9aca4484791a926680`.
Its circuit inventory matches, but three witness/reference source pins were older than the candidate.
The existing builder rebuilt the circuit artifacts for comparison and verified all three keys
against their R1CS and powers-of-tau. It exported and compiled the same verifier runtimes.
A separate bundle now records all five current witness source hashes:
`.local/metropolis/candidate-20261010/bundle-current-source/`.
Manifest SHA-256: `268bf1abe34d738859a20d5db8cee96a30fd4dbb4b2e25b8b5b9da7084d59a4a`.
Source inventory SHA-256: `403006be52840d06b244a4dda7ebbf02942da189709fb87b0601a38140f12e40`.
Pinned staging verified every artifact and compiled all three matching verifier runtimes.
Artifact bytes, verification keys and runtime identities match the original bundle.
The new manifest uses placeholder `https://example.invalid/fixtures/` URLs, not a hosted service.
The pool address remains the rehearsal address, not an authenticated Monad deployment.
Original bundles, package manifests and qualification reports remain unchanged.
No setup entropy, independent trust, live deployment, publication or new package qualification is claimed.
Regenerate domain/URL bindings and repeat installed gates for the final live candidate.

### M5D shared journal path checks (2026-10-10, uncommitted)

Journal reads and lock opens previously followed existing symlinks. The engine now rejects
symlink roots, records, blobs and locks, plus non-regular entries. New transaction, record
and lock files request `0600` at creation instead of relying only on a later chmod.
Four regressions verify root/entry rejection, unchanged outside bytes and permissions,
directory-entry rejection, and dangling-link rejection instead of absence or target creation.
The journal suite passes 28 tests; clippy is clean.
Coordinator lifecycle tests pass 25 tests, with the exhaustive sweep still a separate gate.
The legacy Rust SDK's `--locked --all-targets` suites also pass, including unchanged
journal characterization fixtures. No live Starknet tests were enabled.

The shielded all-targets run reached 82 passing library tests and passing access/artifact,
indexer and metered suites, then failed the fresh shielded-operator CLI test. Inspection
found separately sampled buyer/seller service deadlines in that fixture; one shared deadline
now matches the public-bound fixture. The product still rejects mismatching service terms.
The unmodified CLI suite passed on retry (8 passed, 11 ignored); that retry alone does not
prove the timing defect fixed. The updated fresh-operator test passed in 10.21 seconds;
targeted clippy is clean. Separately rerun remaining targets passed: prover validation 4,
RPC finality 12, wallet 10, witness 2. Wallet chain/fault and x402 HTTP tests remained ignored
in this run. This is split-suite evidence, not a new full funded or release qualification.

No record layout, signing plan, reservation rule or durable-write order changed.
These are existing-path checks, not protection against hostile concurrent namespace changes.
Parent directories and the local owner remain trusted. Files remain plaintext where their
consumer does not encrypt them. The wider file review and final funded sweeps stay open.

### M5D model-visible SDK/MCP result boundary (2026-10-10, uncommitted)

The access seam previously forwarded an arbitrary native error string when its JSON envelope
was otherwise well formed. Recognized metered error codes now map to fixed public messages;
unknown codes fail closed. MCP uses `AccessError.public_failure()` to keep arbitrary exception
messages and codes out of tool results while preserving actionable ledger/exhaustion errors.
Snapshot failures without a code retain generic no-recharge guidance.

Access, proving, negotiation/payment and disclosure wrappers now handle undecodable subprocess
output with fixed recovery messages. The payment regression invokes the subprocess once and
requires retaining the operation ID and recovering by observation; it never retries submission.
Other tests inject synthetic private error detail and confirm it is not returned.

Snapshot resource paths must match the configured cache and issuance ID. Metered paths must
name the cache's fixed calls directory and digest-shaped journal filename. Python checks path
metadata only; Rust still verifies, persists and reads the resource. Cross-root, traversal and
unexpected-name tests fail closed. Hashes, commitments and configured local paths remain
model-visible metadata; these checks do not establish payment, delivery, host trust or anonymity.

Final executed evidence: `uv run --frozen pytest sdk/py/tests mcp-server/tests agents/tests
scripts/tests -q`: 635 passed, 13 skipped in 43.16 s. A separate opt-in run with
`EREBUS_M8_TEST_LOCAL_PROOF=1`, `EREBUS_M8_TEST_ACCESS_INSTALL=1` and
`--with hatchling==1.32.4` passed 120 proving/access SDK/MCP tests in 30.91 s,
including real local proving and funded retrieval through installed seams. These are team-run
local test fixtures, not outside acceptance or live Monad shielded evidence.
`git diff --check` passed. Wider file review and final funded candidate/release gates remain open.

### M5D relay errors, configuration and ciphertext storage (2026-10-10, uncommitted)

Review found that an empty token counted as authenticated configuration and that HTTP
storage errors returned raw internal diagnostics. The relay now rejects empty/blank tokens,
binds unauthenticated development mode to loopback, validates read-query fields and returns
fixed public errors. Actual HTTP tests verify unauthorized writes create no mailbox, invalid
queries do not echo a private marker, and corrupt-index and I/O errors redact private data.

A real Noise exchange now uses `FileRelay`: after reopening the relay, disk blobs and index
entries contain no offer marker, index fields are public metadata only, and files are owner-only.
A different mailbox is empty; another session cannot decrypt the retained ciphertext. The
intended recipient decrypts it and reaches the sender's transcript root. This is local fixture
evidence, not anonymity, per-mailbox authorization or a live deployment. The relay cannot
validate whether arbitrary callers upload encrypted data; direct plaintext uploads are exposed.

Executed: `cargo test --manifest-path sdk/transport/Cargo.toml --locked --all-targets`
passed 97 library, 2 relay, 11 disclosure, 9 integration and 1 example tests. The subprocess
worker is ignored as a standalone test and invoked by the passing process-level test.
All-target clippy with `-D warnings`, formatting and `git diff --check` passed.
The test-only Tokio I/O feature adds `bytes` to Tokio's lock-file dependency list; no
dependency version or protocol format changed. Wider file/model-visible review remains open.

### M5D artifact HTTP and public-indexer boundaries (2026-10-10, uncommitted)

The artifact audit now checks the actual requests received by the fixture server:
GET method, named public files, empty bodies, public headers and no private query data.
Three fresh CLI processes generate real deposit, transfer and withdrawal proofs locally.
The server receives exactly nine artifact requests; the cache contains only authenticated
public artifact bytes and empty lock files. Private witness inputs remain separate local
owner-only test files. Failure assertions no longer print captured prover output.

A two-server redirect regression reproduced exposure of a source URL's synthetic query
token through reqwest's default Referer header. The artifact client now disables automatic
referrers; the same regression passes. Redirect bounds, URL validation, hash checks and
publisher authentication are unchanged. The original host still sees its own URL.

The public indexer now rejects unknown query fields and malformed cursors with a plain
400 status. Its empty-body middleware rejects nonempty bodies and waits at most one second.
A raw chunked request with no first chunk receives 400 before the test's three-second read
deadline. Valid pagination still includes deployment block zero and returns the next page.
These guards cannot prevent an upstream proxy from observing a rejected request.

Executed commands: `cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test
artifacts -- --include-ignored`: 19 passed in 97.28 s; `cargo test --manifest-path
sdk/shielded/Cargo.toml --locked --bin erebus_pool_indexer`: 2 passed in 1.07 s;
all-target clippy with `-D warnings`, formatting and `git diff --check` passed.
The tests use public fixture files and known-entropy local proving artifacts. They do not
qualify a release, prove network anonymity, replace independent audit, or close the wider
relay/file/model-visible review. The threat model now explicitly distinguishes encrypted
note snapshots from owner-only plaintext participant transcripts and witness inputs.

### M5C actual local rollback and explicit replacement (2026-10-10, uncommitted)

`rolled_back_withdrawal_recovers_by_same_intent_replacement_after_restart` passed
in 81.01 s. A real deposit proof seeds a confirmed note. Anvil snapshots the chain
before withdrawal; the withdrawal is mined but the proxy reports finality behind it.
The operation does not complete. `evm_revert` then rolls back the actual payout and
pool state: the recipient balance returns from 7 to 0 and the old receipt disappears.

The note reservation and signer claim stay held. After reopening the durable stores,
an explicit same-intent fee replacement executes the withdrawal. Recovery records
one consumed note, one completed attempt, a 7-unit payout and a released signer claim.
The history retains two distinct transaction hashes. Repeating a completed resume
fails closed; repeating replacement returns Completed without changing the history.

This is actual local state rollback with controlled reported finality, not a reversal
of finalized consensus or a live Monad reorg. CI includes the targeted command.
No product logic changed in this slice. Earlier test runs corrected JSON-RPC envelope
handling and assertions about completed resume and repeated history entries before
the passing run. Targeted clippy, formatting and `git diff --check` passed.
The complete `cargo test --manifest-path sdk/shielded/Cargo.toml --locked --all-targets`
run also passed all non-ignored suites. Ignored funded gates were not all rerun by
that command; the rollback and RPC-audited payment runs above were executed separately.

### M5D funded shielded RPC schema audit (2026-10-10, uncommitted)

The real-proof negotiation/recovery/access/disclosure test passed in 169.35 s with
1,639 bounded JSON-RPC requests and exactly one proof-only broadcast. Both fixture
RPC endpoints record requests in memory and fail the audit if capture is truncated.
They are proxies over one Anvil node, not independent providers.

The auditor restricts method names, parameter counts, public query shapes and call
selectors. Estimation and broadcast must match the selected pool, gas sender,
chain, zero native value and exact proof calldata. It rejects extra witness fields,
private strings, oversized query payloads, changed calldata and private request IDs.
Two auditor unit tests passed; clippy passed for negotiation and wallet-fault tests.
The checker initially rejected omitted parameters on zero-argument RPCs and the
legitimate estimation call; both schema cases were corrected before the passing run.

This tests the exercised flow, not every possible RPC, HTTP header or application
log. Gas sender, timing, asset, proof/public inputs and public history queries remain
visible. Artifact, relay/indexer, file and broader diagnostic exposure remain open.
The chain, participant keys and proving setup are local test fixtures. No live Monad
shielded transaction, independent audit or release qualification is claimed.

### M5D real-verifier seeded conservation sequences (2026-10-10, uncommitted)

`npm run test:stateful-proof` passed all three fixed pseudorandom seeds against local Anvil,
the actual generated test-setup Groth16 verifiers, real Poseidon and `ErebusShieldedPool`.
The script authenticates artifact file hashes against the local manifest before loading them.
It creates its own loopback chain and does not alter the existing generated witness fixtures.

Each sequence deposits two notes, executes four separately authorized private transfers,
reuses positive change, exercises an exact-value transfer with zero change, and withdraws
every remaining positive note. After each transition, a note model checks liabilities,
token supply, paid-out tokens, pool balance, ordered Merkle leaves/root and consumed identities.
Public-input mutations and duplicate deposits/transfers/withdrawals are submitted as actual
reverting transactions; model state must remain unchanged. Hash equality to the local manifest
does not authenticate a production setup or establish trust in its entropy.

Observed results: each seed generated 11 proofs (2 deposit, 4 transfer, 5 withdrawal), for
33 proofs overall. Deposit and final withdrawal totals were 353, 489 and 650 units; all three
vaults ended at zero. Only seed names, counts and totals are printed; no private witness files
are written. Participant material is reproducible test material, not release identities.
The sequence harness constructs circuit authorizations directly; it is not an agent/discovery
or installed-MCP acceptance run. CI now includes the command after artifact generation.
`node --check` and `git diff --check` also passed.

Together with the 64-transition bookkeeping/wrapper fuzz and dedicated callback tests, this
checks the local M5D stateful-test item. Three seeds are bounded coverage, not an exhaustive
property campaign, independent audit, qualified asset behavior or live Monad proof.

Current harness SHA-256 values (not a frozen revision): `scripts/stateful-proof.mjs`
`225d636e882c399b426be92d4e00b5160cdb17fdf056c8b5ee0efce4d79c9a25`;
`scripts/pool.mjs` `8b4e3700c4f011ccdbfe2610c1e6cd82f65dfced12335e485f8f0973754837f4`
(both paths under `circuits/m5`). Circuit source and generated setup keys were not changed.

### M5D funded RPC and legacy regression gates (2026-10-10, uncommitted)

The current source passed these additional executed gates after the diagnostic changes:

- `cargo test --manifest-path sdk/evm/Cargo.toml --locked --test local_chain --
  --include-ignored --skip every_public_bound_operation_write_recovers_one_actual_payment`:
  43 passed in 24.29 s. This covers dropped/timed-out/falsified broadcast responses,
  provider disagreement, history/ancestry interruption, rate limits, reorg observation,
  relayer restart and independent auditor processes on Anvil.
- The separately executed release-mode `every_public_bound_operation` write-boundary sweep
  passed in 15.45 s. Together with the 61.17 s both-mode coordinator sweep above, this is
  current crash-recovery evidence; the generic coordinator sweep does not replace a funded
  shielded contract run.
- `cargo test --manifest-path sdk/rs/Cargo.toml --locked --all-targets`: every non-ignored
  suite passed, including Cairo-reference hashing/decryption, wire versions, journal
  characterization, scoped disclosure, reconciliation and resume. These use pinned Cairo
  fixtures; no new Cairo contract execution, hosted-prover request or mainnet action was run.
- Rebuilt all shielded native binaries, then ran the proving/access SDK and MCP seam suites
  with `EREBUS_M8_TEST_LOCAL_PROOF=1` and `EREBUS_M8_TEST_ACCESS_INSTALL=1`: 105 passed in
  50.15 s. The opt-in tests exercise installed wheels, artifact download, a real local proof
  and funded access. Their setup remains local known-entropy test material, not a published
  candidate release, independent acceptance or Monad private settlement.

The local installed wrap/unwrap implementation checkbox is now checked from its installed
native round trip and every-journal-write fault evidence. Real wrapper/asset qualification
is a separate unchecked gate. Proof-only HTTP sponsorship remains a design/implementation
gap; the existing public-bound relayer must not receive a private agreement opening.

### M5D encoding/evidence diagnostics and downstream regression (2026-10-10, uncommitted)

`Writer` and `PreparedSettlement` now use fixed redacted normal/pretty `Debug` output.
The writer can contain a canonical private agreement; opaque backend evidence can contain
an opening before submission. Their explicit byte APIs are unchanged. New tests verify
redaction and that encoding/decoding and evidence access still work. This complements the
agreement-object fixes below, not an assurance against explicit field logging.

Executed after these changes:

- Core `cargo test --locked --all-targets`: all suites passed, including 81 unit tests,
  pinned canonical vectors and 11 shielded vector tests. Core clippy with warnings denied passed.
- Coordinator `--all-targets`: 25 lifecycle tests passed; its separate release-mode
  `every_discovered_lifecycle_write_is_restartable_in_both_settlement_modes` passed in 61.17 s.
- Transport `--all-targets`: 97 unit, 11 disclosure and 8 Eleusis tests passed, plus its
  transcript example test. The subprocess worker is intentionally ignored except through its parent.
- EVM `--all-targets`: all non-ignored suites passed, including 41 unit and 12 offline-signing
  tests. Ignored funded chain tests were not rerun as part of this command.
- `uv run --frozen pytest sdk/py/tests mcp-server/tests agents/tests scripts/tests -q`:
  618 passed, 13 skipped. Optional installed/native proving checks need their separate gates.
- `git diff --check`: passed.

Current SHA-256 values: `sdk/core/src/encoding.rs`
`84559ee3cf1633057ef2078e0dc05230e0734ad4cbae7759680d7f66c0eb55f7`;
`sdk/core/src/settlement.rs`
`396b649e5a3f1c7cfb96966c510b4b16120edcbbdde67c58608205732179ea7a`.
The tree remains uncommitted and unfrozen. This is local regression evidence, not CI,
release qualification, an audit, asset qualification or Monad private-settlement evidence.

### M5D private diagnostic formatting (2026-10-10, uncommitted)

Review found derived `Debug` implementations exposed agreement amount, service terms and
mapped shielded fields. `AgreementTerms`, `ServiceRecord`, `FeePolicy` and `ShieldedDeal`
now return fixed redacted output for normal and pretty debug formatting. Explicit canonical
encoding, witness generation, hashes and authorizations are unchanged. This prevents this
particular accidental object-formatting leak, not explicit logging of fields by callers.

Executed: the complete core all-targets suite passed, including the new formatting regression,
both agreement suites' pinned vectors and authorization tests. Core clippy passed with warnings
denied. This source revision is not frozen or qualified as a release. Earlier source pins are
historical; current SHA-256 values for the changed modules are:

- `sdk/core/src/shielded.rs`: `540ab5fdccfbf156b941ff9853b7708311e2cd395e0cad03f046c603548d8284`
- `sdk/core/src/terms.rs`: `e08ce5b6607570f56930b513386ab77144b48137639f8af9c2c65ba87eb061d1`
- `sdk/core/src/service.rs`: `4a816f9f2a02edeb66e49c57df415005d90ccf29fc2b7c2a5eb4646c0f4b627c`

The downstream shielded access suite also passed all 14 tests in 26.80 seconds after rebuilding
against the changed core. Other downstream suites still need the final candidate regression run.

### M5D neighboring agreement access isolation (2026-10-10, uncommitted)

`separately_authorized_neighboring_deals_cannot_share_access_or_issuance` constructs two
independently authorized agreements between the same buyer and seller, with different deal
identities and settlement nonces. For both authorization suites, each request authenticates
for its own agreement. Cross-deal requests and a transplanted signature with a changed
commitment fail without creating issuance records. Legitimate requests create distinct
issuance IDs, and a restarted issuer preserves each ID and original issuance time.

Executed: all 14 `access::tests` passed in 26.68 seconds; formatting and clippy checks passed.
These are issuer-boundary tests with internally constructed payment certificates, not chain
payment evidence. The separate funded workflow tests cover observation and payment. This
gate does not close the broader RPC, relay/indexer, file leakage or independent review scope.

### M5D chain-visible shielded payment checks (2026-10-09, uncommitted)

The funded `negotiated_shielded_payment_proves_locally_and_recovers_without_a_second_send`
test now reads the actual transaction and receipt after finalized recovery. It checks the
pool target, `transferPrivate` selector and exact 612-byte static ABI layout: eight proof
coordinates and eleven public inputs. Domain, asset, agreement commitment and deal nullifier
must match the negotiated operation. No additional agreement opening or signature blob is
present in that layout. The receipt must contain exactly two pool `NoteInserted` events and
one `DealTransferred` event with matching commitment/nullifiers. No token transfer event or
other emitter is accepted during the inner payment.

Executed: the ignored funded test passed in 167.66 seconds with a real locally generated
proof, test-only setup artifacts, a dropped broadcast response, exactly one submission,
recipient backup recovery and the existing independent-process disclosure/access checks.
This is Anvil evidence, not live Monad execution or independent security review. It does not
prove relationship anonymity: the transaction sender, asset, expiry, commitments, timing,
public deposits and withdrawals remain observable. RPC, relay/indexer and file-level leakage
need their own review; the broader M5D privacy checklist remains open.

The complete shielded crate non-ignored all-targets suite also passed. The separate ignored
`wallet_faults` suite passed all six tests in 538.78 seconds, covering durable write-boundary
recovery, controlled reorg visibility, unsupported finality and fee replacement including
an older attempt winning. The controlled reorg harness is not a consensus reversal.

### M5D independent tree model and wrapper sequence fuzzing (2026-10-09, uncommitted)

The existing state-machine pool test now runs 64 mixed transitions per fuzz input.
It rebuilds the expected tree root from ordered commitments using a separate level-by-level
model, without reading the pool's frontier or zero hashes. Rejected fee-token deposits and
withdrawals must preserve supply, allowance, balances, leaf counts, commitments and nullifiers.
Later transitions continue to check the independently rebuilt root and exact vault accounting.

`testFuzz_wrapper_sequences_preserve_backing_and_failed_burns` adds 64-transition sequences
of native deposits, transfers, delegated transfers, withdrawals, insufficient allowance,
over-withdrawals, and failed native payouts. Supply equals tracked balances and native
backing after every step; failed burns preserve both token and native balances.

Executed: `forge test` in `contracts/evm` passed all 40 tests, including 256 fuzz runs
per sequence gate and the separate deposit/withdraw callback rollback tests. The local
Foundry toolchain warned that it does not recognize the optional Monad profile settings;
this run uses the default EVM profile, not Monad-native execution evidence.
The pool verifier and hash remain explicit bookkeeping fixtures. These tests do not
establish cryptographic proof soundness, randomized real-proof private conservation,
deployed WMON behavior, or an independent audit. At that run the broader M5D item remained
open; the later real-verifier seeded sequences above complete its local test coverage gate.

### M5C installed random-key asset workflows (2026-10-09, uncommitted)

The candidate now passes both complete installed asset workflows:
`fresh_operators_deposit_negotiate_reuse_change_and_withdraw_erc20` and
`fresh_operators_wrap_deposit_negotiate_reuse_change_and_unwrap`.
They generate identities through product setup, create notes through wallet deposits,
negotiate two 70-unit deals from an initial 150-unit note, reuse retained change,
and recover finalized payments. Repeated settlement requests submit nothing new.
Buyer and seller withdraw exactly 10 and 140 units; the native path also completes
their unwraps. Only the fixture's unrelated 150-unit note remains in each pool.
Participant keys and note openings are random; no source-tree evidence watcher or
fixture-owned spending material is used by participants.

Executed qualification command:

```sh
python3 scripts/check-metropolis-install.py --registry /private/tmp/erebus-m5c-assets-registry-20261009 --rehearse-assets
```

Result: 2 tests passed in 930.08 s. The fresh environment verified all 15 native
binaries and reported `source_imports: false`, `installed_asset_rehearsal: true`,
`live_payment: false`, and `published: false`. Local registry version is
`0.3.0.dev2026100903` (macOS arm64, debug, dirty candidate source), not a public release.
Contracts, setup keys, funding, and chain are explicit Anvil fixtures; this is team-operated
local installed evidence, not qualified WMON/ERC-20 deployments or independent acceptance.

Test corrections followed the existing product contracts: pending finality exits 2,
read-only payment recovery uses `observe`, and an unreserved note is represented as null.
The harness retries only pending or the specific fail-closed paired-pool read error,
within a monotonic deadline, without signing or submitting on the observation path.
Earlier failing/terminated harness runs do not count as passing evidence.
The setup now retains a separate owner-only 64-byte `agreement.public` for wallet
ownership; the 32-byte signing seed remains separate and never crosses the counterparty boundary.
Targeted clippy passed; format and diff checks passed. Package/prover Python tests:
56 passed, 9 skipped. No commit, publication, deployment, or live transaction occurred.

### M5C fresh shielded operator onboarding (2026-10-09, uncommitted)

Candidate `erebus-negotiate` now supports suite-2 `prepare_operator`, private
`prepare_binding`, and suite-2 `prepare_terms`. The operator generates distinct random
discovery and payment keys, transport keys, a wallet encryption key, and a seller
spending secret. Bindings use the existing descriptor-signed deployment format;
they are owner-only counterparty files, not discovery publications. Binding authority
does not establish payment-key possession; the encrypted authorization exchange still does.
No signed agreement or binding format changed.

`fresh_shielded_operators_bind_private_keys_and_negotiate` runs two separate commands
using only freshly prepared material. It verifies the accepted agreement and transcript,
matches both authorizations to the generated keys, and confirms the final payment tag
comes from the seller's secret rather than the template placeholder. Wrong deployment,
missing, wrong-peer, tampered, and mixed-suite bindings fail before template creation.
Existing public-bound setup remains the default.

Executed: `cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test negotiation_cli`
passed 6 tests, with 11 chain/manual tests ignored (50.08 s). Targeted binary/test clippy
with `-D warnings` passed. These are source-command negotiation checks, not installed
full-flow asset checks or chain payment evidence. Published dev5 and the earlier local
prepaid wheel are unchanged; neither includes this new onboarding path.
The complete local installed ERC-20/WMON gate has since passed as recorded above.
Real-asset and live-chain qualification remain open.

### M5C candidate prepaid service integration (2026-10-09, uncommitted)

Selected prepaid modules were ported onto the current candidate-v2 working tree.
This is not a merge of the older prepaid branch's circuits or wallet format.
The current strict EdDSA verifier, domain-bound notes, wallet format v2, change retention,
and durable wallet transaction fences remain in place.

The source now supports `http-metered-v1`: one finalized private payment buys bounded calls
to a seller-configured data feed. The buyer authenticates with its existing agreement key.
The seller certifies the canonical payment location at both pinned RPCs, stores an entitlement,
and persists each original response before acknowledgment. SDK/MCP tools consume or recover
access without payment authority. Exhaustion is HTTP 403, not a recharge challenge.

Review fixes: missing or modified response blobs now fail status and next-call checks closed;
replay fsyncs the call directory; issuer initialization rejects symlinked journal directories,
sets owner-only permissions, and syncs directory creation. Scanning rechecks chain identity
before committing staged history. Payment-certificate Debug output is redacted.
Certification rejects a consumed deal whose matching settlement log is absent.

Verified locally: 81 library tests pass, including the 42-boundary issuer fault sweep.
The Unix directory creation/reopen/symlink regression passes. Twelve finality tests pass,
including bounded rewind after a fork. The independent checker matches four canonical
encodings and ten metered profile/request vectors. Python workspace tests pass: 616 passed,
13 skipped. Source format and all-target clippy checks pass.

An unpublished macOS debug registry, `0.3.0.dev2026100902`, installs outside the checkout.
All 15 native hashes, SDK/MCP imports, and relay lifecycle pass the isolated install check.
This registry comes from dirty source; it is not a clean release or Linux qualification.
The published dev5 packages are unchanged and do not contain prepaid access.
The fresh installed rehearsal passes in 262.67 s. Two negotiation processes agree on five
prepaid calls; installed payment settles once. The service recovers a dropped response and
restart. Buyer recovery restores a deleted cursor through read-only status/replay, with up
to three bounded retries on pending/unavailable replies. Other errors still fail immediately.
The installed buyer command consumes index 4 and explicitly replays that index from cache.
Exhaustion returns 403. Previously committed calls remain available after the chain stops.
The installed auditor verifies agreement/payment from its encrypted grant with both participant
directories renamed offline; it does not verify delivery. Exactly one broadcast is recorded.

Local debug measurements: 1,788,707 settlement gas; first recovered access 20,741 ms;
replayed access 7,159 ms; service state 99,553 bytes; post-activation growth 4,239 bytes;
buyer cache 2,369 bytes. Output is also written to the ignored
`sdk/shielded/target/metered-e2e-measurements.json`. These are not Monad latency figures.

Commands:

```sh
python3 scripts/build-metropolis-registry.py --version 0.3.0.dev2026100902 \
  --profile debug --build --output /private/tmp/erebus-m5c-metered-registry-20261009
python3 scripts/check-metropolis-install.py \
  --registry /private/tmp/erebus-m5c-metered-registry-20261009 --rehearse-metered
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --all-targets
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test rpc_finality
cargo clippy --manifest-path sdk/shielded/Cargo.toml --locked --all-targets -- -D warnings
python3 scripts/check_agreement_vectors.py
uv run --frozen pytest sdk/py/tests mcp-server/tests agents/tests scripts/tests -q
```

The shielded all-target suite passes, including 14 client, nine HTTP, and 11 scanner tests.
Core all-target tests and 11 transport disclosure negatives also pass. The final Python
workspace rerun remains 616 passed, 13 skipped. The access command's protocol-2 SDK/MCP seam
run passes 70 tests with one explicit installed-environment skip.
Legacy Starknet SDK all-target regressions pass. Both funded public-bound/x402 HTTP tests
pass after the access-profile integration (2.65 s). No live provider or prover tests were run.

This integration changes source hashes recorded in the older candidate handoff. Those pins
describe historical verification revisions, not this new working tree. Re-pin the complete
launch scope after a clean approved commit; this does not freeze or audit the candidate.

The service's certificate trusts provider-finalized anchors and retained local state.
Owner-only entitlement records contain private terms and response blobs without journal
encryption. A response-hash check does not authenticate future feed contents.
This profile is not standard x402 compatibility, an audit, or live Monad evidence.

### M5D candidate-v2 constraint mutation gate (2026-10-09, uncommitted)

`npm run test:negative-witness` checks hash-pinned local WASM against generated fixture inputs.
It keeps witnesses in memory and prints only per-circuit counts. It creates no setup keys,
proof files, transactions, or network requests. Loading and schema errors do not count as
constraint rejection. Each circuit must accept its valid input before and after the mutations.

Executed results: deposit rejects 20 mutations, transfer rejects 129, and withdrawal rejects 84.
The 233 cases cover scalar binding, integer bounds, zero required fields, Merkle siblings
and directions, authorization scalars/identity keys, and conserved but unauthorized pricing.
CI now runs this gate after generating the local prototype artifacts.

The Rust proof regression now mutates each of the 25 public inputs and all nine proof
components across the three transitions. It also checks all six mismatched circuit-key pairs.
These checks use test-only local artifacts. They do not establish setup independence or audit coverage.
The final expanded regression passed in 297.90 s. Targeted clippy, Rust formatting,
JavaScript syntax, and `git diff --check` passed.

Commands for this gate:

```sh
cd circuits/m5
npm run prototype
npm run test:negative-witness
cd ../..
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test local_proof -- --ignored
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test witness -- --include-ignored
cargo test --manifest-path sdk/transport/Cargo.toml --locked --all-targets
cargo test --manifest-path sdk/core/Cargo.toml --locked --all-targets
cargo test --manifest-path sdk/coordinator/Cargo.toml --locked --release --test lifecycle \
  every_discovered_lifecycle_write_is_restartable_in_both_settlement_modes -- --ignored --exact
forge test --root contracts/evm
```

The funded prototype also checks a valid private-payment proof at its signed expiry boundary.
Only the Anvil clock advances; the call must reject specifically with `expired` and leave
the deal and note unused. Restoring the snapshot allows the unchanged proof to settle normally.
`npm run prototype` passed deposit, private payment, note recovery, withdrawal, and replay checks
with this expiry test. Its expected negative-witness diagnostics are not test failures.

The both-mode coordinator restart sweep passed in 64.70 s. All three witness tests passed
in 73.00 s, including the Rust/JS comparison and durable preparation tests. Core tests passed
(109 tests, one regeneration test ignored). Transport passed 97 unit, 11 disclosure, eight
Eleusis integration, and one example test; one subprocess worker is intentionally ignored.
Foundry passed 39 tests, including callback rollback and wrapper bookkeeping tests.
The existing unrecognized Monad-profile configuration warnings remain.

The candidate now contains the selected prepaid service/certificate integration described above.
The older prepaid branch's circuit and wallet implementations were not substituted.
Independent audit, real asset qualification, and authorized Monad shielded execution remain open.
Nothing was committed, published, or broadcast outside Anvil.

### M5C installed native-value round trip (2026-10-09, uncommitted)

The local fixture now selects either its existing ERC-20 asset or a WMON asset.
It still refuses non-loopback RPCs and any chain other than Anvil 31337.
The WMON mode deploys an asset-specific pool and funds the fixture note through wrapping,
not a token mint. The fixture's seeded note remains separate from the random participant.

The new installed-wallet regression wraps 0.02 native units, approves the pool,
deposits half as a random shielded note, recovers it, withdraws it, and unwraps the full amount.
All five operations finalize and produce exactly five submission requests.
The participant's native balance returns to its initial value minus the exact receipt fees.
The pool returns to its starting token balance. Completed wrap and unwrap retries send nothing.
This does not exercise a negotiated private WMON payment or external acceptance.

Two defects surfaced before the passing run. The wrapper fixture lacked `transferFrom`;
it now supports finite and unlimited allowances with rollback on insufficient funds.
The wallet incorrectly refused a wrapper equal to its pool asset; that check is removed.
Zero and malformed wrapper addresses remain rejected. Signed formats, circuits, proving keys,
and the production pool contract are unchanged.

The unpublished debug registry is `0.3.0.dev2026100901`, built from the dirty working tree.
A fresh full install verified all 15 binary hashes, external imports, and relay lifecycle.
The native wheel was also installed offline into
`/private/tmp/erebus-m5c-native-installed-20261009` for the wallet tests.
Its `erebus-wallet` SHA-256 is
`5193af458ae48754dccf7d563b5b425280a098fe1bde3ead09778bc0587521c9`.
This is local macOS qualification, not publication or Linux qualification of these fixes.

Executed verification:

```sh
EREBUS_TEST_INSTALLED_BIN_DIR=/private/tmp/erebus-m5c-native-installed-20261009/bin \
  cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test wallet_ops \
  -- --ignored --test-threads=1
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --bin erebus-wallet
cargo clippy --manifest-path sdk/shielded/Cargo.toml --locked \
  --bin erebus-wallet --test wallet_ops -- -D warnings
forge test --root contracts/evm
python3 scripts/check-metropolis-install.py \
  --registry /private/tmp/erebus-m5c-native-registry-20261009
```

Results: two installed wallet tests passed in 217.72 s; three wallet binary tests passed;
39 Foundry tests passed; targeted clippy, formatting, and JavaScript syntax checks passed.
The existing ERC-20 backup/recovery regression remains green after the shared fixture helper extraction.
Foundry still reports the existing unrecognized Monad-profile configuration warnings.
Nothing was committed, published, deployed to Monad, or broadcast outside Anvil.

### M5C installed stale-backup and endpoint recovery (2026-10-09, uncommitted)

The wallet regression now retains an encrypted snapshot before withdrawal. Later,
it removes the current snapshot from the configured path and restores that stale backup.
It moves both history caches aside and selects two new RPC endpoints. The recovery
configuration has no transaction key, manifest file, or proving artifacts.

Separate installed `erebus-wallet sync` processes rebuild finalized history. They recover
the withdrawn note as consumed and the other note as included and unreserved.
Both RPC proxies count zero submission requests. The account nonce and token balances
remain unchanged. The existing operation and signer journals remain available; this
does not prove recovery from missing journals or lost spending keys.

Executed from the repository root:

```sh
EREBUS_TEST_INSTALLED_BIN_DIR=/private/tmp/erebus-m5b-installed-20261008/bin \
  cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test wallet_ops -- --ignored
cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test wallet
cargo clippy --manifest-path sdk/shielded/Cargo.toml --locked --test wallet_ops -- -D warnings
```

Results: the installed wallet regression passed in 121.89 s; ten wallet tests passed,
one was ignored; formatting and targeted clippy passed. The first regression attempt
failed because its new assertion expected `reserved: false`. The CLI represents an
unreserved note as `reserved: null`. The corrected assertion and submission counters pass.
No production code or response format changed.

The installed command comes from the unpublished macOS debug environment recorded below.
Its SHA-256 is `4629a8e9e16e9f4a06632b7144111a519a7cb11252bc7e401df19e9b70e0b2fd`.
The test driver, deployment fixture, and artifact server still use repository code.
Participant and note material are random; pool keys and token/wrapper contracts remain fixtures.
The new endpoints are loopback proxies over one Anvil node, not independent providers.
M5C's local stale-backup recovery gate is checked. Live asset and endpoint qualification,
live prepaid acceptance, and the complete private release remain open. Installed local
prepaid acceptance now passes as recorded above.

### Bootstrap clock and callback hardening (2026-10-09, uncommitted)

The intermittent suite-2 binding failure recorded below is now diagnosed and fixed.
`erebus-negotiate` previously validated a received binding against time captured before
the Noise handshake. A peer issuing its binding one second later could fail that check.
`NegotiationBootstrap::shielded_with_clock` now samples validation time after blocking
receipt and at completion. Both bindings and descriptors must remain valid at completion;
future issuance, expiry, clock errors, and backwards time fail closed. Fixed logical-time
bootstrap remains available for tests. No signed format, circuit, or artifact changed.

Executed verification on the current source:

- Transport: 97 unit, 11 disclosure, eight Eleusis integration tests, and one example
  test pass; one integration test remains ignored. The three new deterministic clock
  tests reproduce the old failure and cover refreshed time, future issuance, expiry,
  failure, and rollback. Formatting, clippy with warnings denied, and doctests pass.
- Shielded: all non-ignored all-target tests pass, including the freeze/resume
  regression; formatting and clippy with warnings denied pass.
- Funded source-command regression: all three shielded-driver tests pass in 530.17 s
  against the pinned single-operator bundle. They cover recipient backup recovery,
  received-note reuse in another negotiated deal, and separate MCP operators recovering
  a lost broadcast response. These are Anvil tests, not live Monad evidence. Earlier
  installed-package evidence below predates this clock fix.
- Foundry: all 36 tests pass. New deposit and withdrawal callbacks attempt a private
  transfer during token movement, require rollback of balances/tree/nullifiers, and
  succeed after disarming the callback. The permissive verifier fixture tests bookkeeping
  and reentrancy, not cryptographic soundness.
- SDK/MCP access-result boundary: 35 tests pass, one is skipped. Retrieval distinguishes
  resource hashes, seller-reported finality, independently verified payment, and delivery.
  It cannot claim independently verified payment or delivery from retrieval alone.
- Setup tooling: 17 tests pass across phase-1/phase-2 verification, test setup,
  bundle preparation, and staging. The three-circuit two-step fixture chain verifies.
  Node reports file-handle cleanup warnings during snarkjs verification; the process
  exits successfully, but these warnings are not evidence of a warning-free toolchain.

The roadmap now checks the implemented manifest-authentication and result-separation
requirements plus these narrow adversarial gates. Independent release setup, audit,
qualified assets, prepaid service integration, and live shielded acceptance remain open.
The existing prepaid branch uses older circuit mapping and is not evidence for the
hardened candidate-v2 wallet or setup until it is ported and retested.

### Repeated private spending (2026-10-09, uncommitted)

`recipient_spends_a_recovered_payment_in_a_second_negotiated_deal` passes with
source-built commands (229.64 s) and installed macOS commands (225.73 s).
The installed commands come from the unpublished debug package environment at
`/private/tmp/erebus-m5b-installed-20261008`; the driver and chain fixture remain
repository test code. Both runs use the pinned single-operator artifact bundle.

The first deal pays 70 units. Its recipient then becomes a buyer, negotiates a
new 65-unit agreement in separate processes, and pays from the received note.
No deposit is seeded into this buyer's wallet. Finalized recovery consumes the
70-unit note, retains a spendable 5-unit change note, and refuses selection of
6 units. Repeated settlement calls for both agreements submit nothing more;
the shared gas-payer nonce lane records exactly two broadcasts. These are local
Anvil results with fixture identities, not private Monad settlement or external
acceptance. The broader M5C installed asset-flow checkbox remains open.

Targeted clippy and formatting pass. During the separate non-ignored negotiation
suite, four tests passed and the freeze/resume test failed at suite-2 identity
binding. That test passed in isolation (49.77 s), but its intermittent failure
was unresolved at that point. The later bootstrap-clock section above records its fix
and the passing full-suite rerun; the earlier isolated run alone was not a green suite.
Two earlier new-test attempts had assertion/request mistakes: a pending submission
was incorrectly required to exit successfully, and an unsupported `recover`
method was used. The final test uses the existing `settle` reconciliation path
and asserts no additional broadcast; production behavior was not changed.

### Recipient backup recovery (2026-10-09, uncommitted)

The negotiated shielded CLI regression now asserts that seller consent follows
durable encrypted payment-note storage. It saves a backup before payment and
confirms that the expected note has no inclusion evidence and is not spendable.
After the payment finalizes, the test removes the original seller wallet and
standalone spending-secret file, hides the buyer's storage directory, and opens
the backup as a fresh wallet store. A fresh public-history cache recovers the
same 70-unit payment note from finalized Anvil evidence without sender storage.
The test then restores buyer storage for its existing delivery and auditor gates;
the whole loop still requires exactly one broadcast.

Executed with the pinned single-operator bundle:
`negotiated_shielded_payment_proves_locally_and_recovers_without_a_second_send`
passed (162.70 s). Formatting and targeted clippy with warnings denied also pass.
An earlier attempt failed because the test backup used a parent directory that
did not meet the wallet's permission policy. The backup now uses the protected
wallet directory; production permission checks are unchanged. This tests local
store reopening and chain recovery, not a live Monad recipient or an independent
operator. M5B documentation and artifact-failure tests are now checked separately
from its still-open independent-ceremony and non-fixture release requirements.

### Single-operator testnet setup (2026-10-08, uncommitted)

Testnet funding preparation: a fresh OS-random gas key was created owner-only at
`.local/metropolis/testnet-gas.key` (gitignored). The Rust `erebus-settle address`
command derived `0x5b6459b961baec43a40c305099365d767793e8ad`. It is reserved for
Monad testnet (10143), not an agreement identity or a deployed pool. No spend budget,
deployment, approval, or broadcast follows from funding this account. The key-preparation
tests verify address derivation, mode 0600, and refusal to overwrite a file or symlink.
Local Anvil tests need no faucet funds. Approved escalation now permits local listeners.
Funding confirmed read-only on 2026-10-08: the official testnet RPC and Ankr both
returned chain ID `0x279f` (10143) and finalized balance `0x8ac7230489e80000`
(10 testnet MON) for this gas address. Nothing was spent. Earlier sandbox-only loopback
probes returned `EPERM`. Foundry `cast balance` panicked in macOS system-proxy discovery;
direct JSON-RPC reads succeeded, so the panic was not treated as funding failure.

Follow-up integration: `bundle-testnet-setup.mjs` reverified the complete contribution
chains, staged the new keys without changing fixtures, rebuilt candidate R1CS/WASM,
and exported matching verifiers and a complete version-2 manifest. Bundle:
`/private/tmp/erebus-testnet-bundle-wWk6Q8/bundle`; manifest SHA-256:
`37fed4d21ea6d68f783a4ccb65879764a9d0487f51397c9aca4484791a926680`.
Its domain matches the retained local candidate witnesses; its test address and
`example.invalid` URLs are not a deployment or hosted distribution.

Installed macOS native-wheel qualification against this bundle: 24 passed, three
source-fixture cases skipped, 113.47 s. All three transitions generated local proofs
from signed manifests; wrong-publisher, signature and manifest-byte mutations were
rejected. This is dirty-source debug packaging with offline cache reuse, not a full
SDK/MCP release or Linux qualification. New bundle rejection gates also pass.
The updated macOS native-wheel suite passes: 31 passed, three source-fixture
cases skipped (119.51 s), including the explicit qualification-timeout tests.
Generated Solidity verifiers passed three Foundry in-process tests, each accepting
a fresh proof and rejecting a changed public input. Verifier source recompilation
checks the manifest's runtime hashes before testing. No RPC or transaction is involved.

Approved local-listener execution exposed a test defect: the timeout case configured
100 ms, below the installer's supported one-second minimum. The test now uses a
one-second timeout and delays the body for 1.5 seconds; production validation is unchanged.
`cargo test --manifest-path sdk/shielded/Cargo.toml --locked --test artifacts` passes:
16 passed, one real-artifact case ignored. This includes interruption/timeout cleanup
and retry, concurrent installation, integrity failures and cached-byte verification.
The existing local Linux x86-64 VM now starts. Its dedicated 16 MiB tmpfs test,
`actual_tmpfs_exhaustion_cleans_partial_download_and_can_retry`, passes (2.00 s).
It reaches real ENOSPC, rejects installation without publishing a partial artifact,
then frees the ballast and successfully installs and verifies all three files.
This is local Linux container evidence, not a remotely executed CI result.
The installed prototype results below close the local qualification work, not the
independent-ceremony or non-fixture release gates.

Funded-harness integration follow-up: an explicit pair of external test-bundle
environment variables now stages checked payloads separately and uses the compiled
verifiers whose runtimes match the manifest. Default fixtures are not changed.
The native fixture helper and coordinator example use the selected artifact directory;
external native proving refuses the stale retained-calldata path. Positive staging
and pool-compilation checks pass against the new keys; corrupt manifests, duplicated
circuits, wrong pins and changed verifier-source metadata are rejected. Rust example
compilation and formatting pass. The earlier sandbox attempt stopped before Anvil startup.
An approved local run subsequently completed with the external bundle and native Rust
coordinated proving: deposit 150, private payment 70, output recovery, withdrawal,
replay/mutation rejection and a separate auditor process without participant state.
Receipt: `/private/var/folders/pf/tp4_k34505bdrjmz5tjzlt1r0000gn/T/erebus-staged-test-bundle-rzjEui/run.json`.
It pins the bundle manifest above, reports `testOnly: true`, final vault balance 80,
`agreement_verified: true`, `payment_verified: true`, `delivery_verified: false`.
This is a funded local source harness, not a Monad deployment or a published release.
The new-key CLI product test also passes (160.05 s): separate negotiation processes,
local proving, lost-response recovery with one broadcast, change recovery, resource
retrieval and independent auditing. The two MCP-agent lost-broadcast test passes
(160.11 s) against the same bundle. These tests copy current binaries into temporary
participant directories; they are not a published-wheel or independent acceptance run.
The native fixture now returns its selected artifact directory to these tests and
the wallet tests, preventing new verifier runtimes from being paired with old keys.
The wallet operations test passes against the same bundle (131.05 s), covering
allowance, deposit/withdrawal recovery and native wrap/unwrap recovery on Anvil.
An unpublished three-wheel debug registry (`0.3.0.dev2026100801`) was then
installed in `/private/tmp/erebus-m5b-installed-20261008`, outside the checkout.
All three imports resolve to that environment; all 15 binaries and the launcher
match the registry manifest. The shielded MCP test now supports the installed
server/Python path rather than always launching checkout modules. Its installed
run passes (169.72 s): separate MCP servers, local proof, one broadcast despite
a lost reply, then finalized observation and a single successful payment.
It downloads 38,434,425 artifact bytes from the local fixture server. This is
team-operated dirty-source debug packaging on Anvil, not publication or external
acceptance. The Python driver and chain fixture remain repository test code.
The separate installed CLI test also passes (163.69 s), including resource
retrieval, positive change recovery and an auditor without participant state.
MacOS targeted clippy and formatting pass. The Linux prover build passes (29m 17s)
without the old probestack linker failure, using
Rust 1.96.0 Bookworm image digest
`sha256:5e2214abe154fe26e39f64488952e5c991eeed1d6d6da7cc8381ae83927f0cfc`.
Only SDK source is mounted read-only; the funded gas key is not mounted.
All three Linux proofs pass with networking disabled, the pinned bundle,
three cache hits and zero downloaded bytes per transition. Binary SHA-256:
`0279bed94ae8f99a0ff65b5d48a7eb910ae7051eaaa71fc1c7170f216353cc53`.
Public metrics from fresh prover children:

| Transition | Elapsed ms | Proving ms | Child CPU ms | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| Deposit | 377,951 | 373,451 | 334,019 | 110,071,808 |
| Transfer | 1,441,847 | 1,425,411 | 1,369,778 | 210,010,112 |
| Withdrawal | 692,493 | 684,037 | 621,366 | 134,660,096 |

Report: `.local/metropolis/linux-qualification/report.json`; SHA-256
`ff0ce342d3777200ed982d96508db15c7f10ff68b9e0ca1fdddc6d15e4b8d5f0`.
This is a copied source-built debug binary under QEMU with concurrent compilation,
not native-hardware performance or a wheel qualification. The first Linux wheel
attempt passed 25 unit tests but
errored in the shared packaging fixture before any installed proof: compression
of the complete debug binary inventory exceeded the 180-second build timeout.
The test now accepts a bounded, explicit wheel-build timeout (default 180 seconds,
maximum 3,600) for emulation. This changes neither the inventory nor proof validation.
The retry built the complete 860,605,683-byte Linux debug wheel, but the first
install was interrupted by host disk exhaustion: QEMU reported `io-error` and
the backing device reported `nospace`. No installed-proof result from that attempt
is accepted. Redundant staging copies and the regenerable macOS shielded Rust
incremental cache were removed; source, binaries, setup keys and receipts were retained.
After the VM stopped and restarted, qualification continued from the hash-checked
wheel, with a full binary-inventory and license recheck before proving.
The recovered run passed six installed cases: deposit, transfer, withdrawal,
wrong publisher, tampered signature and changed manifest bytes. It invoked the
same installed test functions against the retained, verified wheel rather than
rebuilding it. This is not a claim that the interrupted pytest run passed.
Each proof used a fresh environment outside the source checkout, all 15 native
binaries, a verified launcher, three cache hits and zero downloaded bytes.
The installed prover reports `source_imports: false` and `test_only: true`.

| Installed Linux transition | Elapsed ms | Proving ms | Child CPU ms | Peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| Deposit | 179,518 | 177,497 | 302,532 | 110,141,440 |
| Transfer | 998,099 | 982,338 | 1,408,717 | 208,334,848 |
| Withdrawal | 373,081 | 367,799 | 573,523 | 135,561,216 |

Public measurement files are retained under
`.local/metropolis/linux-qualification/linux-recovered-cases/`.
Their SHA-256 identities are:

- Deposit: `33af7ce52b3d323872959a549474a9a9189f0db234548c0ffdc4376e357f0fc6`.
- Transfer: `d393cf76d1235bdb7ca0cbc93185ca6f0043d2e38476e89244ce1767d95df57e`.
- Withdrawal: `ff7b08adfcfae654df7a205999dab941f4adc2f44acedd2eb49d3de03cd14765`.

These are debug-wheel measurements under QEMU, not native Linux hardware or
Monad latency benchmarks. Linux SDK/MCP service qualification, a clean release
build and independent non-fixture acceptance are not established by these six cases.
The qualification helper now accepts an explicit flat build-directory override
without reducing its 15-binary inventory, and removes temporary publisher keys
even when signing fails. Its updated unit suite passes: 26 passed, nine opt-in cases skipped.
The broader scripts suite passes: 188 passed, nine skipped. The install checker
accepts an explicit qualification-only timeout from one to 3,600 seconds, with
600 seconds as the default. This permits slow emulated debug measurements without
changing product proving deadlines, agreement expiry, or proof acceptance.
Clippy for the coordinator example passes with warnings denied. Three artifact
download-sink tests passed: injected write/sync/persist/directory-sync failures,
interrupted/truncated writes, and streamed hash/length mismatches. These are
socket-free deterministic failures, not real disk exhaustion. HTTP recovery is now
covered by the separately executed integration suite above.

The owner authorized automated setup for the testnet prototype. Two agent tasks
implemented and reviewed the tooling; six separate contribution processes used
fresh operating-system randomness. These are one operator's processes, not the
designated independent human contributors. No contributor identity is impersonated.

`testnet-setup.mjs` generated fresh initial keys from the previously verified public
power-16 transcript, added two sequential contributions for each circuit, and
verified all three complete chains. The run exited 0. Outputs are in
`/private/tmp/erebus-testnet-setup-2QtRbV`; receipt SHA-256:
`483740caab3de2cc0a7bca4aa373b7b523609fe95cb9ed6e8641c72d86781e71`.
The receipt explicitly records `test_only: true`, `operator_count: 1`,
`independent: false`, `trust_verified: false`, and `destruction_verified: false`.
The recorded tool hashes still match the current files.

| Circuit | Final zkey SHA-256 |
| --- | --- |
| Deposit | b7923165e48396e7438f34ba1c80d0b66c8fdfcc83eb18cf04534a2b37e362fa |
| Transfer | 5948a8a9c387e1c22f75386b040f159d963c3c5b556e0d7881a166ae7822e9e7 |
| Withdrawal | dcffc7ab8116d2978db5bcb2053f07c66273bc1442159f2a3b4b27fd07247722 |

Fresh JavaScript proofs using these keys and the retained candidate witnesses
verified for deposit, transfer and withdrawal. Mutating a public input rejected
each proof. Local proof-plus-verification durations were 291/1,253/365 ms;
these are not Rust installed-package or Monad latency measurements.
Runner tests: 12 passed. Focused step/chain CLI checks pass, including generic
malformed-JSON errors, symlink and size refusal, and record hash checks. Record
JSON is now hashed and parsed from the same bounded buffer. The complete fixture
bundle regression also passes.

Entropy is generated inside each contribution child, not placed in arguments,
environment, logs, or shared files. Clearing an entropy buffer and terminating
the child does not prove erasure of JavaScript or dependency memory copies.
The output is test-only and must not hold real value. Existing fixtures are
unchanged. No commit, push, publication, deployment, or transaction occurred.
The independent M5B release gate remains open; this run enables a prototype path,
not an independent ceremony, audit, installed qualification, or live settlement.

### M5B executed verification (2026-10-07, uncommitted)

The owner-selected public power-16 transcript passed full local snarkjs verification,
including the published BLAKE2b-512 pin. The command exited 0 and returned
`consistent: true`. Receipt: `/private/tmp/erebus-phase1-public-receipt.json`, SHA-256
`a54c8ad0e9272723e1324f1cbe5e976c1c8420be84f1e79b37b38c0747ea2132`.
The ptau SHA-256 remains
`1c401abb57c9ce531370f3015c3e75c0892e0f32b8b1e94ace0f6682d9695922`.
Checker SHA-256: `f86d1106727e136564f3219e0242873b5690caaa9370a74ea8116944d846de95`;
package-lock SHA-256: `ca06f06bc539c9601c21705b44d7d2662cbb6c4492990cd2d41aa132a89b87f4`.
The receipt does not assert human provenance review, secret destruction, or an audit.
The older interrupted-run and awaiting-output notes below are historical and superseded.

Fixed a distribution defect: the bundle previously omitted the proving files named
by its manifest. It now copies and checks every WASM/R1CS/zkey and verification key,
includes the ptau, pins compiler executables, and rejects source/input/tool changes
before writing the manifest. The full bundle regression passes, including stale
artifacts, symlink/size refusal, changed copies, and no overwrite. It remains fixture-only.

The new whole-chain checker verifies all three circuits from zero-contribution initial
keys, with a common ptau, reviewed R1CS pins, ordered distinct labels, and contiguous
transitions. Its full two-step-per-circuit fixture regression passes; existing inputs
stay unchanged. The temporary contributions are one-operator tests, not Sagar or Sudipta's
work. Human identity, independence and attestations remain external evidence.

Final installed-wheel regression: 24 passed, three source-fixture cases skipped
(136.46 s). All 15 native hashes plus the launcher matched in fresh macOS arm64
environments. Signed version-2 manifests generated deposit, transfer, and withdrawal
proofs; wrong publisher, changed signature, and changed manifest bytes were rejected.
Each proof now runs in a fresh measurement process, excluding prior Node build RAM.

| Circuit | Proving ms | Elapsed ms | Child CPU ms | Prover child peak RSS bytes |
| --- | ---: | ---: | ---: | ---: |
| Deposit | 6,775 | 8,096 | 23,639 | 240,205,824 |
| Transfer | 60,627 | 64,024 | 157,424 | 292,962,304 |
| Withdrawal | 18,727 | 20,326 | 60,763 | 254,672,896 |

Each used three cache hits and zero download bytes. These are dirty-source debug-wheel
measurements with test-only artifacts, not release or cold-download measurements.
Signed fixture manifest SHA-256:
`9ff0d309a5f104c17d19d3907b9441a3b0f0cf65b3b2a0490343c03bbc536616`.
Tests retain only public `public-measurement.json` reports. Python proving seams,
registry and measurement regressions: 79 passed, ten skipped. CI schedules the new
whole-chain and installed checks but has not run on these uncommitted edits.

M5B is still not complete: actual independent phase-2 contributions and Ishita's review,
a frozen release revision, finalized non-fixture keys/verifiers and signed manifests,
and installed non-fixture qualification on both platforms remain. HTTP recovery and
real disk-pressure evidence are also pending; the sandbox socket restriction is unchanged.
No commit, push, publication, deployment, or live transaction occurred.

### Earlier M5B attempts (historical)

M5B next-task handoff (2026-10-07, uncommitted): the owner reports that verification
is complete. The successful output path and exact transcript identity have been
requested, but no new verification record was inspected here. The operations runbook
now specifies the three phase-2 chains, initial-key generation, Sagar then Sudipta's
contributions, six hash-pinned transition checks, and Ishita's independent review.
This is a prepared handoff, not executed setup. The dirty merge remains unfrozen;
no initial release keys or independent contributions have been generated.

M5B public-transcript and participant continuation (2026-10-07, uncommitted): the
public `powersOfTau28_hez_final_16.ptau` was downloaded from the URL linked by the
official snarkjs table. Its published BLAKE2b-512 pin matched. The downloaded SHA-256 is
`1c401abb57c9ce531370f3015c3e75c0892e0f32b8b1e94ace0f6682d9695922`;
the header reports power 16 and original ceremony power 28. The owner approved this
candidate subject to full verification and designated Sagar and Sudipta as contributors,
with Ishita as the independent verifier. No contributions, verification attestations,
or secret-destruction attestations have been received.

`node circuits/m5/scripts/verify-phase1.mjs /private/tmp/erebus-phase1-public-plan.json`
passed the file and published-digest gates but did not return a cryptographic receipt.
The long-running command was interrupted (exit 130), not passed. Snarkjs 0.7.6 computes
the initial challenge at the original ceremony power; the truncated file does not remove
that work. No verification step was bypassed. Full transcript verification remains open.
The downloaded file, local provenance record, and hash-pinned plan remain in `/private/tmp`;
these local paths are not published ceremony evidence.

The phase-2 handoff fixture regression passed without generating a contribution.
Three socket-free download-sink tests passed; packaging/measurement regressions passed
43 tests with six opt-in cases skipped. The HTTP interruption test still fails before
its assertions at sandbox socket binding (`Operation not permitted`). Docker could not
connect to its daemon, so no Linux qualification was executed. M5B remains incomplete;
the owner-approved roles do not substitute for participant execution or release artifacts.

M5B installed-wheel continuation (2026-10-07, uncommitted): cached build dependencies
allowed an offline native-wheel build and fresh installation outside the checkout on
macOS arm64. Each of three fresh environments verified all 15 native binary hashes and
the launcher hash, then generated a deposit, transfer, or withdrawal proof with cached
test-only artifacts. The executed suite passed 16 tests, with three source-fixture cases
skipped (106.21 s). After correcting a reserved pytest parameter name in two additional
scope-refusal tests, the default suite passed 15 tests with six opt-in cases skipped.

Installed proving times were 5,750 / 59,754 / 18,753 ms; elapsed times were
7,052 / 63,111 / 20,282 ms. Child CPU was 23,834 / 154,796 / 60,026 ms.
Cumulative child peak RSS reached 291,160,064 bytes, not a per-transition memory
measurement. Each proof used three cache hits and zero download bytes. The wheel was
a dirty-source debug build, not a publication candidate. Its prover SHA-256 was
`000d4e4ef2b308ccd66824aa6033097736a046f14f301a2a6c76dd1bb5e74add`;
the fixture manifest SHA-256 was
`09dcd1629d536e0704f225ae3b6a40213de7dacbab85c0cd2fc1bf0d6cfeb500`.

The offline checker reports `scope: native-prover-only`: it does not qualify SDK/MCP,
services, cold downloads, Linux, or non-fixture release keys. CI now schedules this
installed-wheel regression alongside source-fixture proving; that CI change has not
run remotely. Independent contributors, approved phase-1 provenance, and reviewed
non-fixture artifacts remain missing. M5B stays open. Earlier same-day entries below
record historical attempts and their limitations.

M5B public phase-1 pin preparation (2026-10-07, uncommitted): the read-only checker now
supports an optional independently supplied SHA-256 or BLAKE2b-512 published transcript
pin in addition to the internal SHA-256 file pin. The operations runbook records the
power-16 candidate from the official snarkjs table, with its published BLAKE2b-512 digest
and provenance references. This candidate is not selected, downloaded, or cryptographically
verified here; existing local fixture verification is not evidence for it. The local wheel
build environment also lacks its Hatchling/build dependency, so installed qualification
was not completed by substituting a source binary. Independent phase-2 participants and
approved non-fixture artifacts remain absent.

M5B ceremony handoff continuation (2026-10-07, uncommitted): read-only phase-1 tooling
verifies the hash-pinned transcript and preserves a provenance-record hash without endorsing
that record. Phase-2 handoff tooling verifies both pinned keys against R1CS/ptau and
requires an unchanged contribution prefix plus exactly one named entropy contribution.
It refuses changed prefixes, skipped steps, a beacon, different labels/circuits, and file
hash mismatches. The fixture regressions passed against the existing phase-1 transcript
and deposit initial/final keys; no entropy or new key was generated and fixture hashes
were unchanged. Receipts explicitly do not authenticate human identity, independence,
secret destruction, or ceremony trust. Phase-1 provenance is still not selected.

The fresh-install checker now optionally accepts `--prover-request`. It measures the
installed binary against its package manifest hash, uses the isolated installation's
working directory/environment, and fails on changed binaries or failed proving. Thirteen
runner/adapter tests passed with three opt-in fixture proofs skipped. Those two adapter
tests mock the child process: they do not prove an installed package generated a proof.
Related packaging/index tests: 49 passed, one failed at sandbox HTTP socket binding.
No fresh install was executed in this follow-up; Linux and non-fixture package qualification
remain open. CI includes the ceremony fixture regressions but has not run on these edits.

M5B download and measurement follow-up (2026-10-07, uncommitted): the installer now owns
partial-file writes and publication through one internal download sink. Three socket-free
tests cover interrupted/cancelled streams, hash/length rejection, and injected storage
failures at write, file-sync, rename, and directory-sync boundaries. After a rename but
failed directory-sync, only fully verified bytes may remain; retries recheck their hashes.
These are deterministic error injections, not a real disk-filling experiment.

The HTTP interruption/timeout-and-retry test is added but failed at socket binding with
`Operation not permitted`; it did not execute its network assertions here. The full library
run likewise had six existing socket-bound failures (29 passed before the third sink test
was added). They are not disabled. The focused artifact suite passed ten tests with one
ignored and six HTTP tests filtered; all-target clippy passed.

`scripts/qualify-metropolis-prover.py` checks binary/manifest hashes and returns only public
metrics. Eleven runner unit tests pass (three explicit fixture cases skipped by default).
An opt-in actual fixture run passed all three transitions using the cached debug source
binary on macOS arm64: deposit proving 5,816 ms, transfer 59,530 ms, withdrawal 19,390 ms;
elapsed 7,097 / 62,271 / 20,985 ms. Child CPU was 24,347 / 155,251 / 59,373 ms. Cumulative
child peak RSS reached 310,329,344 bytes; it is not an individual withdrawal memory reading.
Every transition had three cache hits and zero download bytes. The executed fixture run
had seven runner unit tests plus three proofs (ten passed, 90.70 s); four more unit cases
were added and passed afterward. This is not installed-package, cold-download, Linux,
non-fixture, or ceremony evidence. CI schedules the source fixture measurement, not a
release qualification. Current artifact installer SHA-256:
`a196cfdde457d1e58b6d217b7ac1a8656ac07a9c1e1298a70284fcdfdd621a1f`.

M5B continuation (2026-10-07, uncommitted): the fixture bundle builder recompiles all
three circuits with Circom 2.2.3 and rejects retained R1CS/WASM that do not match. It
checks all zkeys and exported verification keys, exports Solidity verifiers, and compiles
their complete runtime with Solc 0.8.24 and explicit settings. Its version-2 test-only
manifest binds transitive source inventory, toolchain and witness-source hashes, file
identities, domain, and verifier runtimes. The builder refuses non-test bundles and never
generates entropy, contributes to a setup, uploads, or deploys. The executable bundle
regression passed, including stale artifacts, escaped includes, and release-label refusal.

Socket-free artifact suite: ten passed, one ignored, five HTTP-server tests filtered
because this sandbox cannot bind their local servers. New offline tests reject partial,
tampered, wrong-size, and obstructed caches; signed cache reuse remains accepted. Shielded
all-target clippy and `git diff --check` pass. The new CI bundle step has not run remotely.
M5B remains open: phase-1 provenance and independent phase-2 contributions, release-artifact
qualification, disk-full/interrupted HTTP testing, and installed proving on both platforms
are not complete. No ceremony or publication occurred. Existing fixture keys remain unsafe
for a funded release.

Local validation/recovery continuation (2026-10-07, uncommitted): the prover validates
private fields and public expectations before loading the witness engine. Direct artifact
checks reject non-regular paths, zero/oversized files, changed/truncated/missing bytes, and
bound the hash read. Four regression tests pass. The real native deposit/transfer/withdraw
proof test passes (113.57 s) after this change, using the existing known-entropy fixtures.
The expanded native regression also passed (166.69 s): every transition's generated proof
rejects an identity-point mutation and a changed public input under its actual verification
key. This duration covers proof generation plus extra key loading/negative verification;
it is not a per-proof latency measurement or independent security review.

`positive_change_backup_builds_a_new_withdrawal_without_original_wallet_or_seller` passes;
the original wallet is removed after backing up its pre-submission change opening. Fixture
history replay restores spendability. The companion local-proof test also passed, producing
and verifying a withdrawal proof for that 80-unit change (both tests: 19.60 s). This is not
an on-chain payout, authenticated live history, or a ceremony-derived setup. The socket-free
wallet suite passes (ten passed, one ignored). CI now explicitly runs the proof-dependent
backup test; the new step has not run remotely. All-target Rust clippy passes.

Foundry default-EVM tests: 34 passed, including a new 16-step state-machine fuzz test with
256 runs. It checks public vault conservation, roots/leaf counts, replay rejection, and
atomic failure with a permissive verifier, not private-value conservation or circuit
soundness. Installed Foundry warns that the optional Monad profile's `network`, `hardfork`,
and `inherits` settings are unsupported; this is not Monad execution evidence.
Current uncommitted prover source SHA-256:
`cf2772f061628423f7e42b279a22f14043427fd0dce17758449d478308952309`.
Prior verification entries refer to their stated earlier tree, not these new edits.
Deposit/withdraw witness preparation now rejects mismatched chain, pool, or verifier-version
note domains before proving. A regression test covers each mismatch, contradictory version
arguments, and both valid-domain builders. This is SDK validation, not deployed asset qualification.

Artifact-authentication follow-up (2026-10-07, uncommitted source): non-test manifests now
require a detached suite-1 signature from an independently pinned publisher. Installation,
local proving, payment, wallet, Python, and local-prover MCP configuration enforce that boundary.
The publisher signing command reads an owner-only key and returns only the address and signature.
Signatures authenticate exact manifest bytes; they do not certify ceremony entropy or an audit.

Focused verification: two Rust publisher tests passed; Python/MCP proving tests passed
(33 passed, one artifact-dependent test skipped); shielded all-target clippy passed. The full
artifact suite reported six passed, one ignored, and five failures while binding local HTTP
servers (`Operation not permitted` in this sandbox). Those download tests were not verified
here. All three local fixture zkeys verify against R1CS/ptau, and their exported verification
keys match the stored JSON. Fixtures remain test-only. Independent contributors and a security
reviewer are not arranged, as confirmed by the owner; private-release gates remain open.
The setup regression also passed: changing the deposit verification key fails the checker
while its valid zkey still passes consistency checks. The two unchanged keys remain accepted.
CI now runs both setup checks after generating prototype artifacts; that CI change has not run
remotely. No ceremony, deployment, commit, or publication occurred in this follow-up.

The continuation adds release manifest version 2 with signed circuit source-inventory,
witness-schema, verification-key, and verifier-runtime identities. Version 1 is test-only.
The payment command compares verifier identities with its independent live-code pins.
Release-mode wallet configuration now requires pool and dependency pins; authentication
precedes history/receipt reconciliation, and proof preparation checks manifest/domain/pin
consistency. Recovery does not acquire a proving-artifact dependency.

Additional executed checks: eight socket-free artifact tests passed (one proof-download test
ignored and five socket tests filtered after their sandbox binding failures); both wallet pin
tests passed, including release-mode refusal before key/RPC access. Python/MCP proving tests
passed again (33 passed, one skipped); all-target clippy passed. The shielded library run had 27 passed and six
failures binding local access/disclosure HTTP servers (`Operation not permitted`). These
network-dependent tests and live runtime authentication remain unverified here. Source
metadata bindings are publisher assertions, not a reproducible-build or security-audit claim.


M8 is **not complete**. `0.3.0.dev5` is published on the separate Metropolis index, qualified on
both platforms, and verified from a fresh public install; `0.3.0.dev4` remains served by the
cumulative index. Team-operated live Monad testnet payments are recorded on both rails, including
the x402 `exact` Permit2 rail and a full public-bound workflow (2026-10-05), each with seller
restart, resource recovery, and independent auditor `payment_verified: true`. The live public-bound
run used the concurrent-observation, moving-chain catch-up, and agreement-lifetime fixes, which are
now released in `0.3.0.dev5`. Shielded settlement has
**not run on Monad**: suite-2 settlement, scoped disclosure, and the two-agent MCP delivery loop
have local Anvil evidence only, with known-entropy test keys and test-only proving artifacts. The
public-bound and x402 rails expose terms and payment metadata on chain (details below). External
third-party acceptance, hosted operation, and the recorded demo video remain open, so M8 and the
submission gates are not closed.

## Verified evidence

### Monad testnet deployments and live transactions

Chain ID 10143. Contract identity was verified before the address was trusted.

| Item | Value | Record |
|---|---|---|
| `ErebusSettlement` deployment | `0xa5f0c864f434331bef9a7fc5e05450d598d24da4`, verifier version 1, block 67495473 | Deploy transaction `0x6c7b810c837c038753815df985c3b9ce210be3b40df6fab4ec0682e1a271ca65`; manifest `contracts/evm/deployments/monad-testnet.json` |
| Deployment identity check | finalized block `0x1b01c4fac8ce92fa33496b80752a0bdfbed65eb2aeaec6c21aac878b551e2aa6`; runtime read by block hash; four immutable reference ranges masked; 15,652 bytes matched; `verifierVersion()` = 1 | `scripts/check-evm-deployment.py` |
| Deployment recheck | finalized block hash `0x4ba79809c01f237e9ec11f80aae75b9b7ce6f642ed790ff21ac77db9d2c8f4bf`; both runtimes 15,652 bytes; masked bytes matched; `verifierVersion()` = 1 | Read-only artifact identity evidence, not an audit |
| Test token | `0x902f79145059910ef875aecf4187c771b204ea14` | Minted to the buyer and approved for the runs below |
| Live rehearsal runtime pin | identical finalized `ErebusSettlement` runtime at both providers, keccak `0x5fa7befc14141996c531dd21a0c7f6c06cd5767a9e08334a1f61e0dfafadf94c` | Rehearsal `init`/`preflight`, 2026-10-04 |
| Network-check finalized anchor | finalized block 67489534 (`0x0c4bf7b2…`), head 67489537 | `erebus-network-check`, 16/16 checks on `testnet-rpc.monad.xyz` |
| Canonical Permit2 | `0x000000000022d473030f116ddee9f6b43ac78ba3` | x402 `exact` rail |
| Canonical `x402ExactPermit2Proxy` | `0x402085c248EeA27D92E8b30b2C58ed07f9E20001` | x402 `exact` rail |
| Canonical `x402UptoPermit2Proxy` | `0x4020A4f3b7b90ccA423B9fabCc0CE57C6C240002` | experimental `upto` model only |
| Multicall3 | `0xcA11bde05977b3631167028862bE2a173976CA11` | Monad testnet canonical |

| Live transaction | Commitment | Nullifier | Recorded outcome |
|---|---|---|---|
| Public-bound settlement `0x1f7ec208a7b03b835f224ac989cc59c9aee4c33634a2b04498b8394ddede4c09` | `4511793857dd5a2f50e5f3910b034a652127bfababaae9abedeae68c02be1abd` | `3cfeea17e120c471e435534a46036fa6542dbf19b9ef50dfd598628b1e757360` | `PaidFinalized`, `payment_finalized: true`; seller 3,000,000 base units after three settlements, buyer 0 |
| Public-bound independent-process run `0x6169b70f2efc1aa83b75f92b7b05074414cf24dcc83c3509133c707a0fdd1fda` (2026-10-02) | `ca7d4cb1c3752b48812d5e8bb6804f636d5e84f096cc573fad1da8960f03f3a1` | `e96d5a7c337202528a422d83fd5cc86f2a1408441cfb3ae58cf0bd0ab0bc091c` | Auditor `agreement_verified: true`, `payment_verified: true`, `delivery_verified: false`; a deterministic test seller key in that example was later removed from the independent path |
| x402 `exact` live payment `0x8a0272e69188ac3be52783ddfc4d4436496a58f2d5c63d808cd3320a1fa5df43` (2026-10-04) | `fa4628c2c2e57b659f2d36b86babf53ec8689e0d49a1708953e993dd1902a769` | — | Access first `pending`, one retry after seller restart, resource hash verified; auditor `agreement_verified: true`, `payment_verified: true`, `delivery_verified: false`; stages: negotiation 352 ms, first request to resource 12,915 ms. Re-verified 2026-10-05 from a refreshed encrypted grant with participant paths withheld: `payment_verified: true`, commitment unchanged |
| Public-bound full workflow `0x1ba9ddb363a0c916f8e41ca8eca7d4c524d1ba4722bf4ea37ed423547700ff73` (2026-10-05) | `477f9ad5c37010c8379721f2573d8d7c51158805cf465ebe72ac1cfc2a698f33` | `a6d31c388b4c13f8fdedf3ec81ac91e1a300a05bc576c56f134a4a8aadfaeeaa` | `consumedDeals == true`; seller 70, buyer 5; receipt status `0x1`; exactly one broadcast attempt, outcome `Submitted`; 38 access retries after a seller restart, resource hash verified; auditor `payment_verified: true` from the grant and public chain only; stages: negotiation 144 ms, settle + catch-up 976 s, settle-to-finality 1,044 s, first request to resource 876 s |

The earlier public-bound live attempt was stopped before broadcast on a transient paired-auth error;
its agreement expired (`2026-10-04T23:40:07Z`), the coordinator held no broadcast attempt, and
finalized reads showed the deal unconsumed. A fresh replacement reused the same funded participants.
Earlier retained operations must not be reused. All live runs above are **team-operated**, not
independent external acceptance.

### Published packages and build artifacts

| Artifact | Identity | Status |
|---|---|---|
| `0.3.0.dev5` | Built from clean `06a3420` by registry run 37372823994; both platforms and the comparison passed; fresh public install outside the checkout verified imports and all 14 native binaries plus the launcher against the manifest | Published as a GitHub prerelease on `PoulavBhowmick03/erebus-metropolis`; served by the cumulative HTTPS index, which still serves `0.3.0.dev4` |
| `0.3.0.dev4` | Built from clean `e9f10d0` by registry run 37186885901; both platforms and the isolated installed x402 rehearsals passed; four deduplicated wheels plus combined `release-manifest.json` | Published as a GitHub prerelease on `PoulavBhowmick03/erebus-metropolis`; HTTPS index `https://poulavbhowmick03.github.io/erebus-metropolis/simple/`; index links each wheel by SHA-256 |
| `0.3.0.dev1` | `f4714bb`, run 37180669150 | Not publishable: macOS wheel tagged `py3-none-macosx_10_9_universal2` while all 13 native binaries were thin arm64 with `minos 11.0` |
| `0.3.0.dev2` | `cbfc2e5`, run 37182102672 | Both platforms and the comparison passed, but no wheel shipped a license; not published. `171be31` added Apache-2.0 `LICENSE` to every wheel and `THIRD_PARTY_NOTICES` for all 706 linked crates |
| Local staged wheels | `0.3.0.dev20261003`, `0.3.0.dev2026100301`, `0.3.0.dev2026100302` under `artifacts/metropolis-registry-local`, `artifacts/metropolis-registry-local-2`, `artifacts/metropolis-review-fd111a7`, `artifacts/metropolis-review-fixes` | Local unpublished builds; not release evidence |
| Local release builds | `0.3.0.dev20261004` (`source_commit 8a1d365`, `dirty_source: true`); `0.3.0.dev2026100401`; `0.3.0.dev2026100402` (`f4714bba42b6909b860ba04d301ab23e4c3c3899`, `dirty_source: false`) | Isolated install checks verified 14 native binaries plus the self-host launcher, imports outside the checkout, relay health, and installed x402 recovery/auditor rehearsals |
| Wheel contents | Apache-2.0 `LICENSE` in every wheel; `erebus-cli` wheel also ships `THIRD_PARTY_NOTICES` for 706 crates; `erebus-selfhost` launcher; 14 native binaries | Published dev4 |

Qualification commits: `5a76bbd` (host OS tag and `MACOSX_DEPLOYMENT_TARGET=11.0`), `171be31`
(license and notices), `cbfc2e5` (build EVM contracts in the M5 job), `44c754a` (bound shielded
access requests), `2708983` (index every published release). The M5 CI job had failed since at
least `191dbf5` because it never ran `forge build`; run 37182102674 passed `access_http`,
`x402_http`, the x402 audit test, and the MCP agent tests for the first time. Linux x86_64
qualification is CI-only and was not verified locally (the emulated colima VM reset under load).

### Historical pins and local artifact identities

| Pin | Value |
|---|---|
| M0 source HEAD | `df86407ba72b542f5d3a851d14c1d0ce8515d1e5`; protected local main `daf33efc464294a2516700cb066a98214e921a39`; `rustc 1.96.0 (ac68faa20 2026-05-25)`, `cargo 1.96.0 (30a34c682 2026-05-25)`; `erebus-sdk 0.3.0`, edition 2021 |
| M0/M1 lockfiles | `sdk/rs` Cargo.lock SHA-256 `ef686af9742ace528186e84c0b374d276c47788fae5876fd452ba2e1e98933c0` (M0) and `e50a25cfca13b303a3c428a35d9c99f4cf5350723de5a77fe52625f68b20f539` (M1); `sdk/core` Cargo.lock SHA-256 `fb6cced151dbeff63216645ac3c61c8b13a0c176918aa69924cf7fbae4ac7f67`; `erebus-core 0.1.0`; M1 source HEAD `05e6135` |
| M0 upstream reference | `starknet-privacy` checkout `3dfe66fe2b59d7b95709ec719547fa88b8ef63f9`, file `packages/privacy/src/privacy.cairo` (`use_note`, `apply_actions`) |
| M4 fixtures and outputs | `circuits/m4/fixtures/public.json` (eleven pinned public field elements); generated `build/metrics.json` and `build/artifact-manifest.json`; manifest name `m4-prototype-v1`; `contracts/evm` sources `ErebusCodec.sol`, `ErebusSettlement.sol`, `MockERC20.sol`, tests `ErebusVectors.t.sol`, `ErebusSettlement.t.sol` |
| M5 fixtures and outputs | `sdk/core/tests/fixtures/m5-note-vector.json`; funded report `circuits/m5/build/run.json`; `sdk/core/tests/fixtures/agreement-v1-vectors.json`; depth-20 tree, 128 MiB shielded cache limit |
| M8 deployment tooling | `contracts/evm/deployments/monad-testnet.json`; `contracts/evm/deployments.example.json`; `scripts/check-evm-deployment.py`; `sdk/evm/src/readiness.rs` (`erebus-network-check`) |

### Test and runner evidence by milestone

Every named test recorded in the historical records is listed here; commands are in
[Reproducing evidence](#reproducing-evidence).

| Milestone | Named evidence |
|---|---|
| M0 | `scripts/tests/test_release_readiness.py` (12 tests); workflow-trigger and publication-guard assertions across `ci.yml`, `wheels.yml`, `canary.yml`, `published-canary.yml`; settlement enforcement map against upstream `3dfe66fe2b59d7b95709ec719547fa88b8ef63f9` |
| M1 | `original_commitment_cannot_authorize_changed_terms`; ignored vector regenerator `regenerate_vectors`; `scripts/check_agreement_vectors.py`; suite-1 external keccak256/EIP-155 vectors |
| M2 | `two_participants_agree_on_one_transcript`; `a_restart_rehandshakes_and_the_transcript_continues`; `independent_processes_rehandshake_reopen_state_and_agree_on_the_root`; `packaged_http_relay_enforces_auth_and_serves_ciphertext`; `a_replayed_ciphertext_is_rejected`; `a_message_from_another_session_is_rejected`; `a_mismatched_capability_prologue_fails_the_handshake`; `a_descriptor_for_a_different_deployment_does_not_support_the_filter`; transcript/session/message/descriptor/relay/store unit tests |
| M3 | `one_authorized_agreement_settles_exactly_once`; `a_replay_is_rejected_on_chain`; `the_contract_rejects_mutated_terms_and_expiry`; `prepare_rejects_terms_that_do_not_match_their_authorizations`; `an_unrelated_successful_receipt_is_not_settlement_evidence`; `the_rpc_chain_must_match_the_configured_chain`; `prepared_routing_fields_must_match_the_evidence`; Solidity KAT against `sdk/core/tests/fixtures/agreement-v1-vectors.json` plus the contract adversarial suite |
| M4 | `circuits/m4` runner: combined agreement+transfer circuit, offchain and Solidity verifier checks against `circuits/m4/fixtures/public.json`, negative mutations (amount, seller key, accepted root, buyer signature, change, expiry, zero blinding/spend secret, zero recipient tag, changed public payment commitment, replay) |
| M5 | `circuits/m5` funded runner: one `150 → 70 payment + 80 change → 70 withdrawal` flow; deposit amount-mutation, fee-on-transfer, and duplicate-note negatives; transfer witness mutations (service digest, seller/buyer authorization, agreed amount, recipient spend tag, input spend secret, conservation, membership sibling) and all eleven published-input mutations; deal/note replay; withdrawal redirect, amount, fee-on-transfer, and replay negatives; reorg/restart/recovery; transcript-root KAT `cd876d46…`; Rust/JS witness field-for-field matches; `sdk/core/tests/fixtures/m5-note-vector.json`; report `circuits/m5/build/run.json` |
| M6 | `coordinated_settlement_finalizes_commits_and_releases_the_signer`; `a_foreign_winner_resolves_an_unknown_attempt_and_holds_the_unconsumed_nonce`; `crashes_at_durable_boundaries_yield_one_payment_and_recoverable_state`; `relayed_settlement_pays_the_published_fee_recipient`; `the_relayer_fails_over_providers_and_reports_funding_shortfall`; `a_dropped_broadcast_response_stays_unknown_and_reconciles_from_chain`; `a_falsified_or_dropped_rpc_response_is_never_payment_evidence`; `invalid_relayer_signer_or_deadline_preserves_durable_state_without_sending`; `internally_consistent_rpc_histories_can_disagree_about_one_deal`; `rejected_signing_preserves_every_file_across_historical_schemas`; `every_public_bound_operation_write_recovers_one_actual_payment`; coordinator `every_discovered` lifecycle sweep (both settlement modes) |
| M7 | `issuer_selects_evidence_from_durable_state_and_a_fresh_recipient_verifies`; disclosure suite (wrong recipient, wrong issuer, expiry, changed ciphertext, missing/altered transcript, neighboring deal, changed terms, backup recovery); funded native-proof Anvil flow with separate shielded CLI and MCP auditors over 75 funded crash-point trials |
| M8 | `two_mcp_agents_negotiate_and_settle_once_through_a_lost_broadcast`; `two_mcp_agents_negotiate_pay_and_retrieve_public_bound_with_one_send`; `two_mcp_agents_pay_over_x402_through_a_dropped_response_and_restarts`; `funded_http_access_recovers_a_lost_response_after_service_restart_without_a_second_payment`; `x402_http_submits_once_recovers_restart_and_rejects_mutations`; `negotiated_x402_settles_once_recovers_restart_and_audits_from_the_grant`; `auditor_verifies_finalized_x402_payment_from_the_grant_alone`; fixtures `hold_public_agent_environment`, `sdk/evm/tests/negotiation_flow.rs`, `sdk/shielded/tests/negotiation_cli.rs`, `sdk/shielded/tests/support/payment_driver.rs`, `sdk/shielded/tests/support/shielded_driver.rs`, `sdk/evm/tests/x402_vectors.rs`, `sdk/evm/tests/x402_permit2_chain.rs`, `sdk/evm/tests/x402_service_models.rs`, `sdk/shielded/tests/x402_http.rs`, `sdk/evm/tests/network_check.rs` |

Selected recorded counts (local unless stated): M1 `sdk/core` 76 passed/1 ignored and `sdk/rs` 368
passed/7 ignored; M2 56 unit + 8 integration; M3 25 Solidity + 9 unit + 7 integration (all
`--include-ignored`); M6 release-mode local-chain runs of 33 then 36 tests, 27 unit + 12 offline +
7 CLI + 3 non-Anvil in the ordinary run, funded sweeps of 56 public-bound and 68 shielded
write-failure trials (later 75 funded crash points in the M7 combined run), 24 journal, 19
coordinator lifecycle; M7 13 disclosure CLI tests, 57 transport unit + 8–11 disclosure + 8 Eleusis,
219 Python tests/2 skipped, public-bound Anvil 36, coordinator 25 lifecycle; M8 Python workspace 394
passed/4 skipped with three unresolved `scripts/tests/test_demo.py` failures over stale legacy web
component paths and video expectations.

### Measurements (local, not Monad latency)

| Measurement | Values | Boundary |
|---|---|---|
| M4 prototype (Apple M4 Pro, macOS arm64, Node 22.18.0) | 31,818 constraints; 11 public inputs; depth-four tree; witness+proof ≈0.7 s; peak Node RSS ≈1 GiB; 256-byte Groth16 proof; standalone verifier ≈285k gas; prototype settlement ≈403k gas on Anvil 1.5.1 | Proof-only transition, no token custody, deposit, tree update, or withdrawal; `m4-prototype-v1` local setup, not portable, not safe for funds |
| x402 service models (Anvil, chain 31337, `--block-time 1`, canonical runtimes pinned from Monad testnet, 20 requests at price 10, debug build) | Per-request `exact`: 20 settlements, 1,819,836 gas (90,991 per request), 5,000,000 charged at a 250,000 gas limit, p50 1,007 ms. Prepaid: 1 settlement, 89,146 gas, p50 8 ms, 1,006 ms one-time. Batched `upto`: 1 settlement of 17 used units, 110,101 gas, p50 3 ms, 756 ms one-time | Prepaid and batched are experiments in `sdk/evm/tests/x402_service_models.rs` only; `exact` is the product rail. Monad charges the gas limit, so the limit column is the cost model. This does not measure Monad inclusion/finality latency, gas price in currency, or concurrent-buyer contention |
| Negotiated x402 local run (debug) | negotiation 759 ms; permit preparation 17 ms; first observed inclusion 81 ms after submission; finalized verification 290 ms after submission; delivery 742 ms; proof null | Anvil; local monotonic clocks |
| Installed x402 checks | dev2026100401: negotiation 468 ms, permit prep + intentional failed HTTP 25 ms, inclusion 47 ms, finalized 139 ms, delivery 319 ms. dev2026100402: 475/25/51/144/304 ms | Anvil; the permit number is not signing-only latency |
| Public-bound agent loop (debug) | full deal 11.6 s: negotiation 0.3 s, local signing 0.09 s, submission 0.04 s, payment verified 2.3 s after submission, delivery 0.1 s | Anvil; inclusion/finality block timestamps are diagnostics and were removed from harness output |
| Shielded MCP harness (debug) | negotiation 6.7 s buyer / 4.5 s seller; artifact download 3.5 s for 32 MB; proof preparation 52 s; local signing 6.7 s; submission 4.2 s | Anvil, prototype keys and artifacts |
| Live public-bound (2026-10-05) | negotiation 144 ms; settle + catch-up 976 s; settle-to-finality 1,044 s; first request to resource 876 s | Local monotonic stage timings on live Monad; not a latency guarantee |
| Live log-query concurrency | 48 `eth_getLogs` queries: 51.7 s sequential vs 6.4 s at concurrency 16 (8.0x); extrapolated ≈21 minutes of log-query time for the live range | Live Monad public RPC; excludes authentication, ancestry catch-up, retries, and verification |

## Milestone outcomes M0-M8

| Milestone | Outcome | Status |
|---|---|---|
| M0 | Baseline, enforcement map, workflow isolation, and decisions D01-D08 | Complete locally; no live chain or remote CI claimed |
| M1 | Canonical agreement core (`sdk/core`), suite 1, normative spec §13, DM1 decisions | Complete locally |
| M2 | Offchain transport (`sdk/transport`): Noise XX, typed transcript, relay, descriptors | Complete locally; ordered transport only, not business negotiation |
| M3 | `contracts/evm` + `sdk/evm`: public-bound settlement, exactly-once replay state | Complete locally; public payment |
| M4 | Feasibility memo and Groth16/Circom agreement-bound transfer prototype | Complete locally as research/prototype; no production prover |
| M5 | Funded local shielded prototype: pool, depth-20 tree, wallet, indexer, native proof | **Not complete**; roadmap M5 criterion unchecked |
| M6 | `sdk/journal`, `sdk/coordinator`, relayer, observation, recovery, fault sweeps | Implementation locally complete; owner review of open DM6 items |
| M7 | Scoped disclosure: suite-1 and suite-2 grants, offline auditor, backup recovery | Complete at its local gate; Monad and packages were M8 |
| M8 | Monad deployment, packages, live public-bound and x402 runs, local product loop | **Not complete**; external acceptance, hosted services, and demo video open |

**M0 (2026-09-20).** Repository and offline Rust baseline: formatting, tests, Clippy, docs, and
`erebus-cli` build passed; seven live tests remained ignored. The settlement enforcement map
separated local idempotency from contract replay protection, and workflow guards established that
branch pushes cannot trigger packaging or publication. D01-D08 set product, agreement, custody,
compatibility, service, external-acceptance, post-M0 gate, and public-first x402 boundaries. No
commit, push, tag, deployment, or remote CI run was performed.

**M1 (2026-09-20).** A new chain-neutral `sdk/core` crate implemented canonical encoding, ids,
terms, suite registry, commitment, deal identity, authorization, policy, and settlement/receipt
types. Suite 1 is keccak256 commitment plus secp256k1 ECDSA; the shielded suite was deferred.
Review corrections made authorization check the commitment opening, counted fees inside limits,
rejected shielded mode for suite 1, and wired the real STRK20 settlement method through a checked
legacy adapter. The normative record is [metropolis-agreement.md](metropolis-agreement.md) §13
(DM1-1..DM1-6).

**M2 (2026-09-21).** The offchain Eleusis shipped as `sdk/transport`: X25519 identities, Noise XX
sessions with descriptor-bound prologues, canonical envelopes, per-author transcript chains with a
transcript root, a checksummed file store, a ciphertext relay, and signed service descriptors.
Sessions never resume after restart, which removes nonce-reuse risk; the transcript is the durable
object. Metadata (endpoints, mailbox ids, sizes, timing) is explicitly not hidden. M8 later added
typed price-only negotiation (DM8-6) and shielded-key identity binding (DM8-7) without changing the
M1 encoding or M2 hashes.

**M3 (2026-09-22).** `contracts/evm` and `sdk/evm` turned one authorized canonical agreement into
exactly one public payment. The contract re-derives the commitment and nullifier from the terms
opening and both signatures; exact balance deltas reject fee-on-transfer and rebasing tokens; there
is no owner, upgrade, or pause. Asset identifiers are restricted to lowercase
`eip155:<chainId>/erc20:0x<40 hex>`. Public: full accepted agreement, blinding, final signatures,
amount, asset, parties, fee, timing. M6 later removed the unjournaled `submit`/`settle`/`verify`/
`with_confirmations` APIs in favor of coordinated submission and paired finalized reconciliation
(DM6-4).

**M4 (2026-09-24).** The feasibility memo evaluated Monad facts, reusable privacy pools, proof
systems, in-circuit hash/signature options, and proving-key lifecycle; no candidate exposed a
verified hook for an external signed-agreement commitment. The selected prototype owns one combined
agreement-and-transfer relation: BN254 Groth16, Circom 2.2.3/circomlib 2.0.5, Poseidon domain tags
2001-2009, BabyJubJub EdDSA authorizations, reserved suite 2, and a depth-four tree. The prototype
proved and verified locally on Anvil but has no token custody; the setup is single-contributor and
the keys and artifacts are test-only.

**M5 (2026-09-24 to 2026-09-28).** A funded local prototype added the `ErebusShieldedPool` (one
ERC-20 asset, exact deltas, depth-20 Poseidon tree, consumed deal and note nullifiers), deposit/
transfer/withdrawal circuits, a Rust suite-2 mapping with pinned vectors, an encrypted note wallet
with reorg rewind, a verified public event index, and native local proof preparation. Rust and
JavaScript witnesses match field-for-field. The M5 gate remains open: installed/production
artifacts and ceremony, hosted indexer/wallet rehearsal, fee and gas validation, full Monad flow,
and the installed private workflow are unfinished; the npm toolchain had 20 advisories (5 high)
pending review.

**M5A shielded design review (2026-10-06).** Source review of the `metropolis` candidate at
`94aa05902e7b70c2cd14bcc970790c7daba2c0c6` plus the uncommitted M5A working-tree changes.
No commit, deployment, publication, or transaction. Scope: `contracts/evm/src/ErebusShieldedPool.sol`,
`circuits/m5` deposit/transfer/withdraw, the suite-2 core mapping and authorization, the shielded
witness/prover/wallet/indexer/recovery/disclosure/access modules, the coordinator durable
lifecycle, and the installed CLI/MCP paths.

Invariant checklist: invariant → enforcing code → negative evidence. **GAP** marks code without an
in-tree negative test.

| Invariant | Enforcing code | Negative evidence |
|---|---|---|
| Role/domain/terms-bound suite-2 consent; no role substitution | `sdk/core/src/auth.rs`, `shielded_auth.rs`, `transfer.circom` EdDSA | `cross_domain_replay_fails`; `role_substitution_fails`; `revision_replay_fails`; `original_commitment_cannot_authorize_changed_terms`; `shared_agreement_api_matches_the_shielded_circuit` |
| No degenerate authorization key | `shielded_auth::is_identity_key`; `ShieldedDeal::from_terms`; circomlib `8A ≠ 0` | `identity_authorization_keys_cannot_authorize_any_agreement` (added) |
| On-curve and subgroup key validation | `affinidi-zkp-crypto` verify path via `verify_message`; candidate v2 `circuits/m5/lib/eddsa_strict.circom` in `transfer.circom` | `identity_r8_is_accepted_by_both_sides_and_off_curve_r8_is_rejected`; proposal-rig vectors (`torsion_shifted`, identity, order-2, `s_equal_suborder`, `valid`); ordinary witness still verifies |
| Amount bounds and exact conservation | all three circuits; pool `uint128` bounds; exact token deltas | `test_public_input_bounds_and_output_shape_never_consume_state` (added); JS conservation mutation; `testFuzz_exact_vault_delta_for_supported_amounts`; `invalid_note_material_is_rejected` |
| Output ownership and note commitments | `transfer.circom` payment/change notes bound to the verified keys | `rust_builds_the_exact_m5_transfer_witness` (Anvil/artifacts); `native_input_choice_survives_restart_and_keeps_zero_change_opening`; JS witness mutations |
| Recipient note recovery before consent | `OwnedNote::expected_payment`; `erebus_negotiate.rs` persistence before sending consent | negotiation CLI seller-note retention; unit reconstruction tests |
| Merkle membership and root history | circuit paths; pool `knownRoots`; `PoolIndex::apply_block`/`path` | `note_openings_and_paths_match_the_m5_circuit_runner`; `bad_roots_indices_and_parent_hashes_do_not_advance_the_index`; `test_multiple_deposits_keep_earlier_root_available_for_one_spend`; added unknown-root/expiry pool negatives |
| Expiry | `auth.rs` `now >= expiry`; pool `block.timestamp < expiry`; commitment preimage | `expiry_is_enforced_at_and_after_the_deadline`; `test_expired_or_wrong_deployment_transfer_never_consumes_state` (added) |
| Deal and note replay single-use | revision-independent deal nullifier; note nullifier; `consumedDeals`/`consumedNotes` | `revisions_share_one_deal_identity`; `one_authorized_agreement_settles_exactly_once`; `a_replay_is_rejected_on_chain`; `test_deposit_transfer_and_withdraw_update_pool_state_once` |
| Chain/pool/asset/version separation | pool `_checkDomain`; `domain_fields`; `validate_prepared`; deployment binding | `cross_domain_replay_fails`; added wrong-deployment pool negative; `rpc_verification_rejects_unpaid_nonfinal_inconsistent_and_foreign_deployments` |
| Note preimage domain separation | candidate v2 tags 2010/2011 with `noteDomain = Poseidon(chainId, contractAddress, verifierVersion)` in all three circuits and `sdk/core/src/shielded.rs` | `note_openings_and_paths_match_the_m5_circuit_runner` (regenerated vector), `cross_domain_openings_are_not_recognizable`; narrow claim, metadata linkability remains |
| Agreement-to-payment binding (amount, asset, recipient, transcript root, service digest, both authorizations) | suite-2 commitment; `transfer.circom` terms A/B/C and payment note; witness/public-input equality check | `shielded_agreement_requires_both_roles_and_the_exact_opening`; `rust_matches_circomlib_commitment_and_messages`; `shared_agreement_api_matches_the_shielded_circuit`; JS mutation set |
| One-payment fence, retained reservations, observe-only after unknown | coordinator `record`/`broadcast`; shielded `chain.rs`; payment CLI gate; proof-taking policy commit/release | `initial_broadcast_fence_is_atomic_and_never_retries_unknown_history`; `broadcast_restart_preserves_unknown_attempts_and_scopes_late_responses`; `unknown_consumption_holds_and_verified_winner_reconciles_all_revisions_atomically`; `x402_http_submits_once_recovers_restart_and_rejects_mutations` |
| Fail-closed recovery and observation | `recovery.rs` public-index-before-wallet order; paired `observation.rs`; disclosure/access payment checks | `paired_finalized_wallet_recovery_never_writes_on_disagreement_or_peer_failure`; `absent_or_inconsistent_pool_evidence_cannot_release_a_deal`; `absent_unknown_and_nonfinal_payments_are_not_verified` |
| Local witness and proving; no witness upload | `artifacts.rs` download-only installer; `lib.rs` local Groth16 and zeroizing witness | `wrong_bytes_and_truncation_never_publish_an_artifact`; `mismatched_proving_domain_fails_before_installation`; `fresh_process_downloads_real_artifacts_and_proves_locally` (Anvil/artifacts) |
| Service access bound to the finalized agreement; durable, no recharge | `access.rs` policy, request digest, issuance; service persistence order | `wrong_buyer_expiry_commitment_nonce_and_service_never_issue`; `lost_response_and_restart_reuse_one_durable_issuance`; `every_issuance_write_boundary_recovers_without_recharging`; `funded_http_access_recovers_a_lost_response…` (Anvil) |
| Recipient/issuer-bound disclosure with independent payment verification | `transport/disclosure.rs`; `shielded/disclosure.rs` | disclosure suite: wrong recipient, wrong issuer, expiry, changed ciphertext, altered transcript, neighboring deal; `absent_unknown_and_nonfinal_payments_are_not_verified` |

Fixed in this review:

- **M5A-1 (medium, fixed).** Suite-2 verification accepted the identity public key `(0,1)` and the
  publicly computable signature `R8 = Base8, S = 1`, so off-chain agreement/access/disclosure
  signature checks could be forged for an agreement that named the identity key. The circuit
  rejects the identity key (`8A.x ≠ 0`), so no proof could settle it, but Rust and the circuit
  disagreed. `verify_message` and `ShieldedDeal::from_terms` now reject it; regression
  `identity_authorization_keys_cannot_authorize_any_agreement`.

Findings addressed by candidate v2 (implemented in the uncommitted working tree; still audit scope,
not closed by this implementation):

- **M5A-2 (medium, audit).** The circomlib `EdDSAPoseidonVerifier` had no explicit on-curve check
  for `A`/`R8` while Rust checks on-curve and prime-order subgroup membership. Candidate v2 wraps
  both authorization checks in `circuits/m5/lib/eddsa_strict.circom` (on-curve `BabyCheck` for both
  points, prime-order subgroup check for both, non-zero `S`; identity `A` rejected and identity
  `R8` allowed to match Rust). Independent review remains required.
- **M5A-3 (medium, protocol decision).** Note commitments and nullifiers bound `asset` but not
  chain, pool, or verifier version, so different deployments could hold identical commitments and
  nullifiers. Candidate v2 binds `noteDomain = Poseidon(chainId, contractAddress, verifierVersion)`
  into tags 2010/2011 in all three circuits and Rust. The claim is narrow: timing, funding,
  withdrawal, gas-payer, and other metadata remain linkable.
- **M5A-4 (low).** `changeSpendTag` was a free nonzero witness in `transfer.circom`. Candidate v2
  adds `changeSpendSecret` and constrains `changeSpendTag == Poseidon(2006, changeSpendSecret)`
  plus a non-zero secret; the wallet durably retains the positive change opening and recovery is
  covered by restart and encrypted-backup tests.
- **M5A-5 (low).** Core and circuit permitted equal buyer and seller authorization keys. Candidate
  v2 rejects equal keys in `ShieldedDeal::from_terms` as SDK policy; self-deals remain representable
  at the pool level and are documented as not proof of distinct parties.
- **M5A-6 (low, fixed).** The earlier M5 evidence row claimed "256 fuzzed amounts"; no such loop
  exists in `circuits/m5/scripts/pool.mjs`. The row now lists the negatives that do exist; the
  256-run fuzz is the contract state test `testFuzz_exact_vault_delta_for_supported_amounts`.
- **M5A-7 (informational; decision recorded).** The shielded path stays self-submitted with a
  circuit-fixed zero fee, so the buyer's gas address is linked to its settlement. No sponsor or
  unconstrained fee is introduced; any compensation change requires agreement/circuit binding, new
  vectors, new setup, and review (DM6-8).

Asset policy recorded for the release candidate: candidates are an externally deployed, qualified
ERC-20 and a network-authenticated WMON wrapper; each token gets its own immutable pool; the signed
agreement names the actual WMON asset; MON enters and exits only through explicit wrap/unwrap, and
unwrapping exposes amounts and addresses. A direct native-MON pool is out of scope. Fixtures remain
adversarial inputs, not qualification evidence.

Reruns on 2026-10-06 (local, dirty M5A tree): `sdk/core` all-targets (79 unit plus integration
suites), `sdk/coordinator` (25 lifecycle plus unit), `sdk/transport` all-targets, `sdk/shielded`
all-targets (72 passed, 15 ignored Anvil/artifact tests), and `contracts/evm` `forge test`
(33 passed, including the added pool negatives). Nothing was committed, deployed, signed, or
broadcast.

M5A acceptance is **not closed**: most invariants map to code and a negative test, and candidate v2
implements the M5A-2/M5A-3/M5A-4/M5A-5 remediations in the uncommitted working tree, but several
SDK error branches still lack local negatives (deposit/withdraw library branches, malformed Groth16
points, artifact fault paths), and no independent audit exists. The candidate v2 working tree is
the review candidate, not a release.

### M5A-2/M5A-3/M5A-4/M5A-5 candidate v2 (approved 2026-10-07; implemented, integrated, not frozen)

The owner approved C1-C4. The text below keeps the original source-backed proposal, followed by
the executed implementation evidence. No commit exists; a ceremony must not run until this
candidate is committed, re-frozen with published hashes, and independently audited.

**Integration record (2026-10-07).** The candidate working tree was integrated with
`origin/metropolis` at `a5dd7a2` (five commits: `6443604` procurement scenario with auditor
review, `fc58afe` procurement/paid-API docs and F58, `426bdc3` real disclosure flow over the MCP
servers, `47e062c` PR #37 merge, `a5dd7a2` disclosure audit-driver hardening). Merge base is
`94aa059`; the remote commits touch only `README.md`, `agents/**`, and `docs/friction.md`, which
do not overlap the candidate paths, so the merge was conflict-free. It is staged as an
uncommitted `--no-commit` merge (`MERGE_HEAD` = `a5dd7a2`) with every candidate change preserved;
`git merge --abort` is the reversible fallback.

**1. Circuit EdDSA constraints versus Rust (source-backed).**

- `circuits/m5/node_modules/circomlib/circuits/eddsaposeidon.circom`:
  - Scalar bound: `Num2Bits(253)` plus `CompConstant(2736030358979909402780800718157159386076813972158567259200215660948447373040)` and `compConstant.out*enabled === 0` (line 51). `CompConstant` returns 1 when its input is greater than the constant, so the circuit enforces `S < subOrder`; the constant is `subOrder - 1`. There is no `S != 0` constraint.
  - Key check: `8A` is computed with three `BabyDbl` instances and the identity is rejected with `isZero.out*enabled === 0` on `8A.x` (lines 70-83). This rejects the identity and every small-order/torsion point, because `8T = identity` for the whole 8-torsion subgroup.
  - No on-curve check: `BabyCheck` appears only in `pointbits.circom:109`; it is not instantiated by `EdDSAPoseidonVerifier`, `EscalarMulAny`, or `EscalarMulFix`.
  - Cofactored verification: `A` is multiplied by 8 before the `h` scalar multiplication, so the equation verifies under `8A`, and a keyholder can produce a fresh signature for `A + T`. `R8` is not cofactored, but equality against the prime-order left side implicitly rejects any torsion it carries.
  - `h = Poseidon(R8x,R8y,Ax,Ay,M)` is expanded with `Num2Bits_strict` (full 254-bit range), so the circuit multiplies by the full field value.
- Rust (`affinidi-zkp-crypto 0.1.0` through `verify_message`): rejects `S == 0` and `S >= subOrder`; checks `A` and `R8` on-curve; checks prime-order subgroup membership for both. Exact identity handling: the dependency has no identity rejection for either point; `erebus-core` rejects identity **A** (`is_identity_key`) and has no R8 identity check, so Rust accepts an identity R8 when the equation holds (the keyholder can produce such a signature) and rejects identity A. Rust is strictly stronger except that it is not cofactored: it rejects torsion-shifted keys the circuit accepts.
- Executed probe (in-tree `circuits/m5/scripts/eddsa-probe.circom` and `eddsa-probe.mjs`; compile with circom 2.2.3 and run with snarkjs/circomlibjs):
  - `valid` → WITNESS_OK; Rust accepts (cross-language test `circuit_probe_vectors_show_the_exact_circuit_rust_divergence`).
  - `torsion_shifted` (`A' = A + (0,-1)`) → **WITNESS_OK** in the circuit; Rust rejects the same vector. The probe recomputes both the challenge and `S` for `A'`, so this demonstrates that the circuit accepts a **non-prime-order public key**; it does not show that a signature made for `A` verifies unchanged under `A'`, and it is not a demonstrated forgery.
  - `identity_forgery`, `order2_key` → rejected at the `8A.x` check (line 83).
  - `s_equal_suborder` → rejected at the scalar bound (line 51).
  - `off_curve` with an arbitrary signature → rejected at the equation; this does not prove every off-curve witness fails.
  - `s_zero` → no explicit constraint; the arbitrary probe failed at the equation. Unproven risk.
- Demonstrated failures: torsion-key acceptance (circuit vs Rust), identity/small-order rejection, scalar bound. Unproven risks: an off-curve satisfying witness and an `S = 0` satisfying signature.
- Revised candidate C1: an `eddsa_strict` verifier that adds, around the circomlib verifier, `BabyCheck(A)` and `BabyCheck(R8)` (on-curve), a prime-order subgroup check for both points (`subOrder * P == identity` via `EscalarMulAny` with the constant scalar), and a non-zero `S` check; the circomlib scalar bound and identity/torsion `8A.x` rejection remain. This matches the Rust acceptance set: `0 < S < subOrder`, on-curve, prime-order subgroup, non-identity.
  - Measured cost (`circuits/m5/scripts/eddsa-strict.circom`, probe rig): 8,086 constraints for the circomlib verifier alone, 12,729 for the strict verifier, so **+4,643 constraints per verifier instance**. `transfer.circom` instantiates two verifiers (buyer and seller), so the transfer circuit grows by roughly 9.3k constraints (40,138 → about 49.4k, +23%). Deposit and withdrawal never verify EdDSA and are unaffected.
  - Tested with the existing vectors: `valid` → WITNESS_OK; `torsion_shifted` → rejected at the subgroup check; identity and order-2 → rejected by `8A.x`; `identity_r8` → WITNESS_OK for both the plain and strict verifiers, matching Rust (test `identity_r8_is_accepted_by_both_sides_and_off_curve_r8_is_rejected`, which also shows off-curve R8 rejected); `s_equal_suborder` → rejected by the scalar bound; the two unproven cases remain as stated.
  - Full transfer circuit with the strict verifier (`circuits/m5/scripts/transfer-strict.circom`, a proposal probe; the candidate file is untouched): **49,424 constraints versus 40,138** (+9,286, about +23%), matching the two-verifier estimate. The ordinary `build/transfer-input.json` witness (the JS flow pinned byte-exact to Rust by `rust_builds_the_exact_m5_transfer_witness`) generates successfully against both the original and strict transfer WASM, so ordinary buyer and seller signatures still verify. Local snarkjs `fullProve` with known-entropy measurement keys: original 1,059/815 ms, strict 957/918 ms over two runs, so local proving time was not materially changed by the added constraints. Setup/key size and witness size grow (strict witness 1.58 MB vs 1.28 MB); the Groth16 proof size and the public-input count are unchanged. This measurement uses known-entropy local keys and is not release evidence.

**2. Note commitment and nullifier domain separation (C2).**

- Current preimages bind `asset` only: `noteCommitment = Poseidon(2007, asset, amount, ownerAx, ownerAy, spendTag, salt)` and `noteNullifier = Poseidon(2008, spendSecret, note)`. The payment note binds the deployment transitively through `paymentSalt = Poseidon(2009, domain, nonce)`, but deposit and change notes do not.
- Proposed: `noteDomain = Poseidon(chainId, contractAddress, verifierVersion)` computed in-circuit from the existing public inputs; `noteCommitment = Poseidon(2010, noteDomain, asset, amount, ownerAx, ownerAy, spendTag, salt)`; `noteNullifier = Poseidon(2011, noteDomain, spendSecret, noteCommitment)`.
- Chain/pool/version binding: the domain element uses the same values the circuits already range-check and the pool checks on-chain, so no new private witness is needed and the domain cannot be supplied inconsistently.
- Cross-deployment commitment identity: without it, an identical opening produces identical commitments and nullifiers in every deployment, so the same opening is recognizable across pools, versions, or chains. Domain binding prevents identical openings from producing identical commitments and nullifiers across deployment domains. It does not provide general unlinkability: timing, funding, withdrawal, gas-payer, and other metadata remain linkable, and a small anonymity set still permits inference.
- Replay and upgrades: theft was already prevented because trees and funding are per-deployment, but a redeployed pool with the same opening is linkable; the domain removes that. Changing the note preimage changes a funded deployment's note set, so the candidate is a fresh-genesis revision.
- Migration: there is no on-chain value in the prototype. The proposal is a new genesis for a new revision; local wallets must be rebuilt, and the wallet format version is bumped from 1 to 2 so old files fail closed instead of being misread. If a funded deployment existed, old notes would need the old circuit and verifier retained as a separate legacy rail; that is out of scope here.
- Affected: all three circuits, Rust `note_commitment`/`note_nullifier`, the JS runner, the `m5-note-vector.json` and `agreement-suite2-vector.json` fixtures, the byte-exact witness tests, and tree roots. The canonical agreement encoding does not change because agreements do not carry note openings.

**3. M5A-4 change ownership (C3).**

- `changeSpendTag` is a free non-zero witness; Rust derives it from the change spend secret. A fabricated tag can only brick the prover's own change and cannot affect conservation or other parties (the prover must know a tag preimage to spend it later).
- Recommendation: enforce it in candidate v2 by adding `changeSpendSecret` as a private witness and constraining `changeSpendTag == Poseidon(2006, changeSpendSecret)`. The circuit constraint alone does not guarantee recoverable change: the wallet must durably retain the positive change spending secret and salt in its encrypted snapshot before proving and broadcast, and recovery must work from those retained secrets alone after a buyer restart or backup restoration, without the original process and without the seller. Recovery without spending material is not promised. The existing `prepare_transfer_witness` + `reserve_transfer` ordering already persists a positive change before proving and the restart/reservation tests cover it; the v2 requirement is explicit: add a regression test for restart and encrypted-backup restoration of a positive change opening followed by a spend using only the retained secrets. Alternative: record an explicit documented allowance instead. Decision requested.

**4. M5A-5 self-deal semantics (C4).**

- Core and circuits permit `buyer_authorization_key == seller_authorization_key`; a single key can author both roles.
- Recommendation: reject equal keys in `ShieldedDeal::from_terms` (Rust only; no circuit change) and document that this is an **SDK policy, not a pool-enforced invariant**. The contract has no participant registry and cannot enforce party distinctness; the circuit only constrains the keys that appear in the commitment. A self-deal moves the owner's own note to a key the same owner controls: it takes no value from any third party and is deliberately allowed at the pool level (it is also indistinguishable from a legitimate two-party deal to a chain observer). Its risk is representational: an SDK-verified or disclosed deal is not proof that two distinct parties exist, so the access/marketplace layer must not treat it as such. Alternative: record an explicit documented allowance and leave core unchanged. Decision requested.

**5. Candidate v2 change set and compatibility costs.**

- C1 strict EdDSA wrapper (on-curve A/R8, prime-order subgroup for both, non-zero S, identity A rejected and identity R8 allowed to match Rust; +4,643 constraints per verifier instance; measured +9,286 in transfer, 49,424 total; ordinary witnesses still verify; local proving time unchanged at ~0.9-1.0 s on known-entropy measurement keys), C2 note-domain tags 2010/2011 in all three circuits and Rust, C3 change-ownership constraint with durable encrypted retention of the positive change opening and restart/backup recovery, C4 core equal-key rejection as SDK policy plus a documented self-deal allowance.
- Compatibility costs: every circuit and verifier is regenerated, all proving keys and manifests change (this is why the decision precedes any ceremony), the note and witness vectors are regenerated, the byte-exact witness tests and JS runner are updated, the wallet format version becomes 2 with no automatic migration, and the prototype genesis changes. The canonical agreement encoding, the pool contract interface, the public input counts (6/11/8), and the suite-2 authorization mapping are unchanged.
- Requested: owner approval of C1-C4 (or explicit modifications), after which the candidate is implemented, tested, and frozen before phase-1 provenance and independent phase-2 contributions are organized.

**6. Candidate v2 implementation and executed evidence (2026-10-07, known-entropy local setup; not release evidence).**

- C1 implemented: `circuits/m5/lib/eddsa_strict.circom` (on-curve `BabyCheck` for A and R8,
  prime-order subgroup check for both via constant-scalar `EscalarMulAny`, non-zero S) now wraps
  both authorization checks in `circuits/m5/transfer.circom`. Proposal-rig vectors behave as
  specified: `torsion_shifted` rejected at the subgroup check, identity A and order-2 rejected by
  the circomlib `8A.x` check, `identity_r8` accepted by both circuit and Rust, `s_equal_suborder`
  rejected by the scalar bound, `valid` accepted. The core cross-language regression
  (`identity_r8_is_accepted_by_both_sides_and_off_curve_r8_is_rejected`) still passes.
- C2 implemented: `noteDomain = Poseidon(chainId, contractAddress, verifierVersion)`,
  `noteCommitment = Poseidon(2010, noteDomain, asset, amount, ownerAx, ownerAy, spendTag, salt)`,
  `noteNullifier = Poseidon(2011, noteDomain, spendSecret, noteCommitment)` in all three circuits
  and in `sdk/core/src/shielded.rs`. The regenerated `m5-note-vector.json` pins noteDomain
  `13d7f9b2…`, input root `0dfc614b…`, and full root `14792b9a…`; the new negative test
  `cross_domain_openings_are_not_recognizable` passes. The narrow claim is unchanged: metadata
  linkability is not addressed.
- C3 implemented: `changeSpendSecret` is a transfer witness and `Poseidon(2006, changeSpendSecret)
  == changeSpendTag` plus a non-zero secret check are in-circuit. Rust builds the field
  (`sdk/shielded/src/lib.rs`) and the byte-exact cross-language witness test
  (`--test witness -- --ignored`) passes against the JS input. The wallet persists a positive
  change opening before proving (reservation ordering unchanged) and snapshot format v2 keeps the
  domain-bound opening; restart/backup recovery is covered by the non-ignored wallet regression
  `encrypted_backup_restores_note_and_replays_after_reorg` and by the ignored `wallet_faults`
  restart/replacement suite (6 tests).
- C4 implemented: `ShieldedDeal::from_terms` rejects equal buyer/seller authorization keys as SDK
  policy; self-deals remain permitted at the pool level and are documented as such.
- Wallet format v2: `WalletDomain` now carries `verifier_version`, the AAD is 40 bytes
  (magic, chain id, pool, verifier version), snapshots declare version 2 and reject version 1, and
  every stored note carries its `note_domain`; snapshot validation rejects notes outside the
  wallet domain. There is no automatic migration; old files fail closed.
- Executed local measurements with the candidate artifacts (Anvil 1.5.1, Apple M4 Pro, macOS
  arm64, Node 22.18.0):
  - Constraints (`snarkjs r1cs info`): deposit 2,327; transfer 50,343 (11 public, 68 private,
    50,355 wires, 112,006 labels); withdraw 14,141 (8 public, 48 private). The strict verifier is
    the dominant transfer cost.
  - Artifact sizes: WASM 2.97/5.97/4.08 MB; R1CS 0.36/8.35/1.95 MB; zkey 1.14/24.11/6.22 MB for
    deposit/transfer/withdraw.
  - JS snarkjs `fullProve` + offchain verify: deposit 236 ms, transfer 978 ms, withdraw 294 ms.
  - Rust local proof of all three transitions plus input-mutation rejections: 100.73 s wall,
    peak RSS 337,543,168 bytes (~322 MiB).
  - Anvil gas used: deposit 1,122,288; `transferPrivate` 1,788,671; withdraw 335,662. These are
    local gas observations, not Monad gas and not a verification-cost claim for a real ceremony.
  - `npm run prototype` passes end to end (deposit 150 → private payment 70 + change 80 → output
    recovery → withdrawal, with replay and mutation rejections); artifact hashes are recorded in
    `circuits/m5/build/run.json` and `artifact-manifest.json` under the ignored `build/` tree.
- Integration fixes found by the integrated gates: `cargo fmt` applied across all Rust crates
  (the candidate files were unformatted); `#[allow(clippy::too_many_arguments)]` added to
  `sdk/evm/src/chain/rpc.rs` `reserve_nonce_for_call_with_value` to match the crate's existing
  convention; and `sdk/shielded/tests/support/shielded_driver.rs` now strips the fixture's
  `wmon` field before building the `deny_unknown_fields` payment config, which fixed the two
  `payment_driver` shielded tests.
**7. Integrated-tree verification record (2026-10-07).**

Reported/rerun on the dirty no-commit merge state: `HEAD` `94aa059`, staged
`origin/metropolis` `a5dd7a2`, unstaged candidate v2, `MERGE_HEAD` set. These are local reruns
on that state, not CI and not release qualification. Commands and observed results:

```sh
# Per Rust crate: cargo fmt --check; cargo clippy --locked --all-targets -- -D warnings;
# cargo test --locked --all-targets (or --locked / -- --include-ignored as CI specifies); docs.
sdk/rs        cargo test --all-targets: 9+12+16+13+3+6+21+10 passed
sdk/core      cargo test --locked --all-targets: 79+1(ign)+12+4+3+10+6 passed
sdk/journal   cargo test --locked --all-targets: 24 passed
sdk/coordinator cargo test --locked: 25 passed, 1 ignored;
              cargo test --locked --release --test lifecycle every_discovered -- --ignored: 1 passed
sdk/transport cargo test --all-targets: 94+11+8+1 passed
sdk/evm       cargo test --locked -- --include-ignored --skip every_public_bound_operation_write_recovers_one_actual_payment:
              41+13+43(1 filtered)+12+7+5+3+1+4+1 passed;
              cargo test --locked --release --test local_chain every_public_bound_operation -- --ignored: 1 passed
sdk/shielded  cargo test --locked --all-targets: all suites passed (negotiation_cli 5, wallet 8,
              witness 12, wallet_faults 2 non-ignored + 6 ignored, plus lib/bin suites);
              --test witness -- --ignored: 1 passed; --test local_proof -- --ignored: 1 passed;
              --test artifacts -- --include-ignored: 9 passed; --test wallet_ops -- --ignored: 1 passed;
              --test wallet_faults -- --ignored: 6 passed; --test access_http -- --include-ignored: 1 passed;
              --test x402_http x402_http_submits_once_recovers_restart_and_rejects_mutations -- --ignored: 1 passed;
              --test negotiation_cli payment_driver -- --include-ignored: 9 passed
contracts/evm forge build; forge test: 33 passed, 0 failed
Python        uv run pytest -q: 548 passed, 4 skipped;
              EREBUS_M8_TEST_LOCAL_PROOF=1 uv run --locked pytest sdk/py/tests/test_proving_seam.py mcp-server/tests/test_proving.py: 27 passed;
              EREBUS_M8_TEST_ACCESS_INSTALL=1 uv run --locked pytest sdk/py/tests/test_access_seam.py mcp-server/tests/test_access.py: 36 passed
circuits/m5   npm run prototype: passed (plain);
              EREBUS_M5_NATIVE_PROOF=1 EREBUS_M5_RUST_SCAN=1 npm run prototype: passed;
              EREBUS_M5_NATIVE_PROOF=1 EREBUS_M6_COORDINATED=1 EREBUS_M6_MATRIX=1 EREBUS_M8_ACCESS=1 \
                EREBUS_TEST_MCP_PYTHON=<repo>/.venv/bin/python3 npm run prototype: passed
              (M6 74/74 funded boundaries, M7 independent shielded disclosure, M8 access recovery)
git diff --check: clean
```

Link check: the README references to the removed `docs/evidence.md` were fixed to
`docs/status.md`; the remaining broken local links are pre-existing legacy links in
`docs/release-preamble.md` (three `docs/runs/...` paths that repeat the `docs/` prefix) and
`docs/runbook.md` (three `./local-prover.md` links), all present on `HEAD` and left unchanged.

**8. External gates for owner review (not started).**

None of the following is authorized by this document, and no outreach or engagement is implied.

- **Phase-1 provenance.** Needed: an owner decision selecting an existing widely reviewed
  powers-of-tau transcript (or authorizing a new phase-1), its published transcript hashes, and
  the `snarkjs powersoftau verify` evidence; plus the owner's budget/publication decision. No
  transcript is selected.
- **Independent phase-2 contributors.** Needed: two or more named independent contributors (not
  the release author), on separate machines and keys, authorized to contribute per circuit; each
  contribution hash and a passing `snarkjs zkey verify` after every step and finalization; and
  each contributor's toxic-waste destruction attestation. No contributor is named or authorized.
- **Independent circuit/contract audit.** Needed: a named independent auditor or firm engaged by
  the owner, with a scope covering the three circuits and setup provenance, `ErebusShieldedPool`,
  the suite-2 mapping and authorizations, the shielded witness/prover/wallet/indexer/recovery/
  disclosure/access paths, the coordinator durable lifecycle and one-payment fence, the installed
  CLI/MCP seams, and artifact-manifest trust; plus the signed report. No auditor is engaged.
- **WMON/ERC-20 qualification.** Needed: an owner decision on the qualified ERC-20 and the
  authenticated WMON wrapper per chain; confirmation of the WMON ABI/event topics against the
  deployed runtime (the read-only recon recorded runtime keccak `0xa32df4f1…38b6` but the wrapper
  ABI is not confirmed); and qualification evidence for fee-on-transfer, rebasing, pause, and
  blocklist behavior. No asset is qualified.
- **Monad deployment.** Needed: owner authorization to deploy, a funded deployer key and budget,
  a signed deployment manifest binding chain, pool, verifier version, and artifact hashes, a
  post-deploy identity check (`scripts/check-evm-deployment.py`), and the published
  external-acceptance run (D06) with no private assistance. Not started.

**M5C-1 installed wallet operations (2026-10-06).** Added `sdk/shielded/src/operations.rs` and
the `erebus-wallet` binary. `approve` grants the pool an ERC-20 allowance through the shared
fenced signer lane; `deposit` checks the onchain allowance, creates a fresh random note opening
from the operating system, persists it in the encrypted wallet before proving, installs
hash-pinned artifacts, proves locally, and sends at most one transaction. `withdraw` syncs
verified finalized history, reserves the note, proves, and sends at most one transaction. A
durable per-operation journal records the signing plan, transaction hash, and exact raw bytes
before network I/O; after an unknown broadcast the automatic path sends nothing and only an
explicit `resume` may resend the retained bytes. `sync` replays finalized history through two
RPCs, applies finalized canonical receipts, completes operations whose insertion or consumption
is observed, and releases a signer nonce claim only when the finalized account nonce proves
consumption. A finalized reverted transaction clears the retained bytes and returns the
operation to a retryable plan; a reverted withdrawal also releases its reservation. Deposit
completion needs observed insertion; withdrawal completion needs observed consumption and
refuses to release the reservation otherwise. Evidence: ignored Anvil test
`installed_wallet_deposits_random_notes_and_withdraws_one` (installed approval, a proving deposit
that reverts on fee-on-transfer and is reconciled from a finalized receipt, retry with the same
note opening, two deposits of the same amount with different random commitments, verified
inclusion, one withdrawal, verified consumption, exact balances), unit tests
`operation_journal_is_idempotent_and_fences_bytes`,
`call_encoders_bind_the_pinned_selectors_and_signal_counts`,
`approval_identity_binds_chain_pool_asset_and_amount`, and
`a_withdrawal_reservation_concludes_only_with_verified_consumption`. Not yet installed:
wrap/unwrap, receipt-driven completion for approved-but-unobserved effects, gas/allowance
diagnostics beyond the deposit refusal, and the broader wallet backup/reorg/endpoint-switch
matrix. The roadmap M5C items remain open.

**M5C-2 native value and wrap/unwrap (2026-10-06).** The signed transaction path now carries a
native value in `TransactionParams`, `SigningPlan` version 2, `SignedTransaction`, the nonce
claim, retained-byte validation, and broadcast. Version 1 plans remain readable and mean zero
value unambiguously; version 2 is written going forward and is never rewritten as version 1.
Only a wrap may carry nonzero value, and it must equal the wrapped amount; settlement, approval,
deposit, withdrawal, and unwrap require zero. `erebus-wallet` gained `wrap` and `unwrap`
operations with an explicit `operation_ref` bound into the identity, targeting the configured
wrapper with exact calldata. Completion verifies the finalized canonical receipt's target,
value, and calldata hash plus the measured wrapper balance delta (mint for wrap, burn for
unwrap) for the participant; receipt success alone never completes. The operation journal now
persists the expected target, value, amount, and calldata hash and keeps a bounded attempt
history, so a finalized revert clears the active bytes but preserves the reverted transaction
hash and block; a completed operation is never rebroadcast. The lane review also tightened
`record_signed` to bind the exact signed call and generalized the signer claim to bind value.
Evidence: the extended ignored Anvil test
`installed_wallet_deposits_random_notes_and_withdraws_one` exercises an installed approval, a
proving deposit that reverts on fee-on-transfer and is reconciled from a finalized receipt and
retried with the same opening, two random deposits, one withdrawal, a lost-response wrap that
completes by observation with one onchain execution, a wrap, a failed native payout that reverts
and then succeeds on retry, an over-unwrap revert, a wrong-wrapper revert, and an insufficient-
funds refusal; unit tests cover wrapper identity/value binding, revert history across restart,
and a durable write-boundary fault sweep. Local `MockWMON` is an adversarial fixture and does
not qualify Monad's deployed wrapper. Not yet installed: gas/allowance diagnostics beyond the
refusals, and a real wrapper/token qualification (external evidence).

### Independent audit handoff (preparation, 2026-10-06)

An AI review is not an audit. No auditor is engaged and no report exists. This section prepares
the exact handoff; it makes no security claim.

- Candidate revision: base `94aa059` plus the uncommitted candidate v2 working tree, integrated
  with `origin/metropolis` `a5dd7a2` as an uncommitted no-commit merge. No commit exists. Pin by
  file hashes: `circuits/m5/deposit.circom` `bfd098a9…`, `transfer.circom` `b6f3d0c5…`,
  `withdraw.circom` `1eea28d2…`, `circuits/m5/lib/eddsa_strict.circom` `b12d56cd…`,
  `circuits/m5/scripts/pool.mjs` `6843a8ee…`, `native-fixture.mjs` `dc96c3e0…`,
  `regenerate-note-vector.mjs` `6960ce4c…`, `sdk/core/src/shielded.rs` `c81e0509…`,
  `sdk/shielded/src/wallet.rs` `905c962b…`, `sdk/shielded/src/lib.rs` `cf2772f0…`,
  `sdk/shielded/src/operations.rs` `4fd420f4…`, `sdk/shielded/src/chain.rs` `a829bfb0…`,
  `sdk/core/src/shielded_auth.rs` `c56569e0…`,
  `sdk/evm/src/chain/rpc.rs` `d012bdf4…`,
  `sdk/shielded/tests/support/shielded_driver.rs` `143513b9…`,
  `contracts/evm/src/ErebusShieldedPool.sol` `5dbbf028…`,
  `sdk/core/tests/fixtures/m5-note-vector.json` `6207d148…`,
  `sdk/core/tests/fixtures/agreement-suite2-vector.json` `fe4d0d9c…`, plus the test files named
  below. These hashes cover a working tree that is not frozen; pin again after any commit.
- Scope for review: the three circuits and their setup/provenance; `ErebusShieldedPool`; the
  suite-2 mapping and authorizations in `sdk/core`; shielded witness/prover/wallet/indexer/
  recovery/disclosure/access; the coordinator durable lifecycle and one-payment fence; the
  installed CLI/MCP seams; artifact installation and manifest trust.
- Invariants and evidence: the M5A invariant checklist in this document and the
  [threat model](metropolis-threat-model.md); positive and negative suites listed under M1-M8;
  the wallet-operation unit tests and ignored Anvil tests named in M5C-1..M5C-3.
- Known findings: M5A-1 identity-key forgery (fixed, regression present); M5A-2 circuit EdDSA
  on-curve asymmetry (addressed in candidate v2 by the strict verifier, still audit scope);
  M5A-3 note-preimage deployment domain (addressed in candidate v2 by tags 2010/2011 with
  `noteDomain`; narrow claim, metadata linkability remains); M5A-4 under-constrained change tag
  (addressed in candidate v2 by the `changeSpendSecret` constraint plus durable retention, still
  low-severity audit scope); M5A-5 key distinctness (addressed in candidate v2 as SDK policy in
  `ShieldedDeal::from_terms`; self-deals remain allowed at the pool level); M5A-7 gas-payer
  linkability (declared). Wrapper event authentication assumes the WETH9 `Deposit`/`Withdrawal`
  topics; the Monad WMON ABI is not yet confirmed. "Addressed" here means implemented and locally
  tested in the working tree, not audited or closed.
- Setup assumptions: all reviewed keys are single-contributor, known-entropy fixtures. There is
  no ceremony, no independent contribution, and no published non-fixture key. Any trusted-setup
  statement is out of scope until M5B runs. The candidate v2 artifacts under
  `circuits/m5/build/` (hashes in `run.json`/`artifact-manifest.json`) are local test-only and
  change on regeneration; they are not ceremony output.
- Unresolved risks for the reviewer: circuit/setup soundness under a real ceremony; wrapper and
  ERC-20 behavior qualification; reorg safety beyond the controlled finality-lag harness; wallet
  replacement fee policy is local and a node may still refuse a replacement;
  disclosure and access binding under adversarial services; coordinator owner-review items
  (DM6); absence of any live shielded Monad execution.

### Asset qualification reconnaissance (read-only, 2026-10-06)

Corrected finality record. An earlier reconnaissance note claimed Monad RPCs expose no
`finalized` tag. That was a diagnostic parsing failure, not provider behavior: `cast rpc`
unwraps the JSON-RPC envelope and prints the result object, while the probe parsed for a
`"result"` key. Re-running the exact method shows all three providers return a finalized block:

- `testnet-rpc.monad.xyz`: `eth_getBlockByNumber ["finalized", false]` returns a full block
  object (exit 0, no error); `cast block finalized --field number` returned `68756211` and later
  `68756328`.
- `rpc-testnet.monadinfra.com` and `rpc.ankr.com/monad_testnet`: same method returns full block
  objects.
- One malformed probe form, `cast rpc eth_getBlockByNumber '["finalized", false]'`, sends the
  array as a single parameter and the provider answers `-32602 Invalid params`; this is an
  argument-shape error, not tag support.
- A real provider disagreement was observed: provider A's finalized height `68756256` had hash
  `0xd8fca686…e262` while provider B returned `0xc44eee0a…e2af` for the same height. The next
  shared height `68756328` (hash `0xca85d842…75d3`, covered by A's finalized `68756328` and B's
  finalized `68756329`) agreed on both providers.

Shared **provider-finalized** anchor (not a consensus proof, and never a confirmation count):
chain 10143 block `68756328`, hash `0xca85d842ddb574247152e8ab44cbf248cf17a3528e1b6002f7acd4440cbb75d3`,
agreed by `testnet-rpc.monad.xyz` and `rpc-testnet.monadinfra.com`, each under its own
`finalized` tag. Provider agreement is checked but is not substituted for the explicit finalized
evidence; the paired client paths recheck anchors and fail closed on disagreement.

| Asset | Address | Runtime keccak256 (both providers at the anchor) | Observed metadata |
|---|---|---|---|
| WMON | `0xFb8bf4c1CC7a94c73D209a149eA2AbEa852BC541` | `0xa32df4f1226f1bb0d6ce9b917752ed687857899a35ebf902c9992edb73b138b6` | "Wrapped MON", 18 decimals, totalSupply 4375000101079403363890998; native balance equal exactly at the anchor |
| USDC | `0x534b2f3A21130d7a60830c2Df862319e593943A3` | `0x96215e6049ed615cdc22fea7701e85458a8e51a2df3e7d45e8f9fa1d521b5a78` | "USDC", 6 decimals, totalSupply 9227583930416912 |
| WETH | `0x45477f4709771331db81944A5E20eF95Bc7BA2D7` | `0xd0a06b12ac47863b5c7be4185c2deaad1c61557033f56c7d4ea74429cbb25e23` | "Wrapped Ether", 18 decimals, totalSupply 0 |

- WMON read-only evidence: `eth_call` simulations of `deposit()` with value and `withdraw(0)`
  succeed from an overridden account; `owner()` and `paused()` return no decodable data. Its
  runtime contains `deposit()` `0xd0e30db0` and `withdraw(uint256)` `0x2e1a7d4d`. Selector
  scanning is unreliable (the USDC `owner()` selector was not found by the scan while the call
  succeeds), so these are supporting observations only.
- WMON implementation authentication (read-only): Monad **mainnet** WMON
  `0x3bd359C1119dA7Da1D913D1C4D2B7c461115433A` and the testnet WMON are byte-identical
  (same 3,249-byte runtime and keccak `0xa32df4f1…38b6`). The explorer publishes the verified
  source (`contract WMON`, `pragma solidity 0.5.17`, optimizer off with 200 runs, default
  evmVersion). Recompiling that source with `solc 0.5.17+commit.d19bba13`, optimizer off,
  reproduces the deployed **executable** bytes exactly: the first 3,206 of 3,249 runtime bytes
  (all executable code and constants) match, and only the trailing 32-byte `bzzr1` metadata hash
  differs. The complete metadata-bearing runtime was **not** reproduced, so exact source-text
  provenance remains an open gap; the explorer's verification is a claim this review did not
  independently pin. The published source is a WETH9 fork with `Deposit(address indexed dst, uint wad)`,
  `Withdrawal(address indexed src, uint wad)`, `deposit()` minting to the sender, `withdraw`
  burning before a `msg.sender.transfer`, `totalSupply() = address(this).balance`, and no owner,
  mint, pause, blacklist, fee, or upgrade path. This confirms the wallet's WETH9 event topics and
  mint/burn assumptions against the published implementation and its executable bytecode, with
  the metadata-hash provenance still open. No `Deposit`/`Withdrawal` events were observed in the
  500 blocks sampled around the earlier anchor.
- USDC is Circle FiatToken v2 with admin powers confirmed by read-only calls: owner
  `0x7A154EA9156D354504ad9A401380AA548039BE8b`, masterMinter
  `0x87f2e95621D8f12b83bb4a3E9975c0eAd524D437`, blacklister
  `0xA5a9723c4D658a6F92701ce3A5246Fa7d4723c26`, pauser `0x7A154EA9156D354504ad9A401380AA548039BE8b`,
  `currency()` "USD", `version()` "2", callable `paused()` and `isBlacklisted(address)`. Minters
  can mint and the owner can configure them; the blacklister can block transfers and the pauser
  can halt them. These are material admin risks for a shielded pool asset.
- Verified source is not available through the attempted public routes: Sourcify v2 returns
  `match: null` for chain 10143 (v1 removed), the Monadscan API endpoint returned
  "Invalid API URL endpoint", and Monadvision is Cloudflare-protected. Transfer-behavior probes
  (fee-on-transfer, rebasing, blacklist, pause) and the WMON event ABI still require an
  authorized behavioral probe, verified source, or pinned-source bytecode reproduction.
- Qualification is incomplete and no asset is qualified. Token-list presence, matching runtime
  hashes, provider agreement, and equal backing are supporting observations, not proof of
  immutability or correct behavior, and provider-finalized agreement is not consensus finality.

**M6 (2026-09-28 to 2026-10-01).** M6 added the shared `sdk/journal`, the local `sdk/coordinator`(durable intent, reservations, signer nonce claims, authorization fences, immutable signing plans),
EVM offline signing, bounded and paired observation, the public-bound relayer, a JSON-RPC fault
proxy, and crash/reorg/false-history sweeps in both settlement modes. Supported operator recovery
requires two matching RPC providers before accounting changes; this is a two-of-two consistency
policy, not Byzantine consensus. Open items are recorded below (DM6-1 owner sign-off, legacy
timestamp provenance, expiry caps and ledger scope, relayer quote/replacement handling). The
shielded path is self-submitted because the circuit fixes fee = 0, which links the buyer's
gas-paying address to the settlement (see [friction.md](friction.md)).

**M7 (2026-10-01 to 2026-10-02).** Scoped disclosure reached its local gate.
`verify_selected_agreement` replays only the selected deal and checks the transcript root,
commitment, nullifier, and both role authorizations. Grants are encrypted to a separate X25519
disclosure key: version 1 uses a suite-1 identity, version 2 signs directly with the participant
suite-2 key (domain `EREBUS_DEAL_DISCLOSURE_V2_SUITE2`, Poseidon tag 3001). The auditor verifies
payment through its own configured RPCs; a seller receipt or grant claim is never accepted as
payment evidence. Delivery remains explicitly unverified, and expiry is verifier policy, not
revocation. Monad deployment and package-only installation remained M8 work.

**M8 (2026-10-02 to 2026-10-05).** `ErebusSettlement` was deployed to Monad testnet and its identity
verified against the reviewed artifact. Native negotiation, payment, access, disclosure, local
proving, MCP, and self-hosting components were built and tested on Anvil, then packaged in a
separate registry; `0.3.0.dev5` is published and fresh-install verified, and `0.3.0.dev4` remains installable. Live team-operated runs
completed the x402 `exact` rail and the full public-bound workflow, both with restart recovery and
independent auditor payment verification. M8 is still not complete: no independent external
acceptance, no hosted services (Render files are prepared and tested only; paid provisioning is not
authorized), no demo video, no live shielded Monad settlement, and the concurrent-observation,
moving-chain catch-up, and lifetime-validation fixes are released in `0.3.0.dev5`.

## Decisions appendix

### M0 decisions D01-D08

| ID | Decision | Outcome |
|---|---|---|
| D01 | Product, modes, and custody | Monad testnet developer release with SDK, CLI, MCP, and self-hostable services. Public-bound and shielded stay distinct; a signed privacy requirement is never downgraded. Clients hold keys and authorize locally; the shielded EVM backend proves locally. Mainnet and real value remain separate release decisions. |
| D02 | Agreement and replay identity | Bilateral authorization of a blinded commitment to canonical versioned terms; one deal-wide settlement identity with a fresh 256-bit nonce per revision. First valid signed revision to settle consumes the deal; later counters do not revoke earlier signed permission. Cancellation is local only; cross-chain uniqueness is not promised. |
| D03 | Authorization roles and policy | Separate transport, agreement, note-spending, and submission keys; bind buyer and seller roles; persist the accepted commitment before submission. Policy limits live below the LLM; the signature suite and commitment hash were M1/M4 gates, and generic cross-key signature verification was rejected. |
| D04 | Compatibility and state | Keep Starknet codecs, records, and CLI behavior under regression; add versioned EVM records instead of rewriting history. Namespace state by backend, chain, deployment, and local identity; legacy STRK20 agreement enforcement is client-side, and a wrapper does not upgrade proof guarantees. |
| D05 | Hosted and self-hosted services | Native clients on Linux x86-64 and macOS arm64; Linux containers for services with persistent volumes and health endpoints. Relay, transaction relayer, and indexer have documented trust and availability boundaries; relay retention defaults to seven days; logs stay free of payloads, keys, and openings. Resource and rate limits were M2/M6 gates before hosting. |
| D06 | External acceptance scenario | A ten-step synthetic, published-instructions-only external run: install, discover, enforce policy, fund, negotiate, shielded Monad payment, recovery without a second payment, access recovery, one-deal disclosure, backup restore. Any private assistance counts as a failed acceptance step; no external participant means the gate stays open. |
| D07 | Decisions gated after M0 | M1 agreement suite, M2 transport, M3 EVM contracts, M4 proof/pool, and M8 shared-service launch each require a decision record plus evidence before dependents proceed. They are not permission to weaken guarantees. |
| D08 | Public-first execution and x402 shape | Complete the public-bound Monad flow before shielded research blocks agent integration, while stating that amount, recipient, asset, signers, and timing stay public. Private x402 is a use-case hypothesis; compare per-request, prepaid, and batched models before naming anything x402-compatible. |

### M1 decisions DM1-1..DM1-6

Normative record: [metropolis-agreement.md](metropolis-agreement.md) §13.

| ID | Decision | Outcome |
|---|---|---|
| DM1-1 | Canonical encoding | One self-delimiting form with strict decode, mirroring the existing `RequestBinding` discipline. Implemented and independently reconstructed by `scripts/check_agreement_vectors.py`. |
| DM1-2 | Suite registry | Suite 1 = keccak256 + secp256k1 ECDSA; the shielded suite was deferred and later reserved as suite 2 for the M4/M5 prototype. Only the experimental `sdk/shielded` backend advertises suite 2. |
| DM1-3 | Shared core crate | `sdk/core` is compile-time isolated from Starknet dependencies; `cargo tree` confirms no Starknet/Felt dependency. |
| DM1-4 | Expiry semantics | Reject at `now >= expiry`; cancellation is local only and restated before signing. |
| DM1-5 | Policy windows | `(now - window, now]`, inclusive limits, reservations held until committed or released; uncertain payments never free capacity early. |
| DM1-6 | Receipt states | Payment and delivery are separate states; unknown finality maps to reconciliation, never to a second payment. |

### M2 decisions DM2-1..DM2-9

| ID | Decision | Outcome |
|---|---|---|
| DM2-1 | Session protocol | Noise XX (`Noise_XX_25519_ChaChaPoly_BLAKE2s`, `snow`). Ephemeral sessions bounded by 4096 messages / 16 MiB plaintext; never resumed after restart; re-handshake creates fresh mailboxes. HPKE wrappers and hand-rolled crypto were rejected. |
| DM2-2 | Transport identity and peer authentication | Two layers: Noise static key on the wire plus a suite-1-signed `ServiceDescriptor` mapping to the agreement identity. M8/DM8-7 added an encrypted descriptor-signed binding for separate suite-2 agreement keys. |
| DM2-3 | Message envelope | Canonical envelope with `protocol_version`, delivering `session_id`, 16-byte `deal_id`, revision, author, contiguous sequence, parent hash, type, and a ≤8192-byte body. `Debug` redacts bodies, session ids, and key material. |
| DM2-4 | Transcript and root | Per-author hash chains under domain tags `EREBUS_MESSAGE_V1`, `EREBUS_TRANSCRIPT_LINK_V1`, `EREBUS_TRANSCRIPT_ROOT_V1`; the root is order-independent across directions. Amended in M5: transport uses transport-versioned keccak256 (`TRANSCRIPT_HASH_VERSION = 1`) regardless of agreement suite, so suite-1 roots are byte-identical. |
| DM2-5 | Relay and durable storage | Minimal authenticated ciphertext relay; mailbox ids derived from session id and role; idempotent `put`; 64 KiB blob, 256 blob, and 7-day default bounds; checksummed records with interrupted-tail recovery and owner-only permissions. The transcript persists before acknowledgment. |
| DM2-6 | Service publication and discovery | Signed portable descriptors with no transcript, reservation price, or deal data; configured directory endpoint; verify signature, expiry, and capabilities; suite 1 with shielded mode or cross-chain assets is rejected. M8 later bounded this to a price-only profile for typed negotiation. |
| DM2-7 | Limits | Fixed wire bounds (8192 body, 65536 envelope, 4096 messages/deal, 4096 messages/session, 16 MiB/session, 64 KiB blob, 256 blobs, 7-day retention, 4 endpoints, 16 assets). Nothing is silently truncated. |
| DM2-8 | Module boundary | Chain-neutral `sdk/transport`, no Starknet/EVM/proving dependency. M2 added no CLI or MCP wiring; M8 built the native negotiation command on it. |
| DM2-9 | Metadata exposure | Plaintext is hidden; endpoints, mailbox ids, sizes, timing, session/message counts, availability, and the fact of communication are not. An observer correlating endpoints and timing can still reconstruct a relationship graph. |

### M3 decisions DM3-A..DM3-H

| ID | Decision | Outcome |
|---|---|---|
| DM3-A | Contract re-derives | The contract recomputes `Cdeal` and `Ndeal` from the terms opening and both signatures; the adapter is not trusted. |
| DM3-B | Exact balance delta | Fee-on-transfer, rebasing, and short-delivery tokens are rejected. |
| DM3-C | Signature format | 65-byte `r\|\|s\|\|v` with raw recovery id, low-`s`, non-zero `r`/`s`, and `ecrecover` to the role key. |
| DM3-D | No administration | No owner, upgrade path, pause, or privileged function; only the constructor chain id and verifier version; changes require a new deployment. |
| DM3-E | Asset form | Exactly `eip155:<chainId>/erc20:0x<40 lowercase hex>`; no aliasing or silent normalization. |
| DM3-F | Live chain check | Contract and adapter both verify the live chain id, preventing cross-chain replay of old domains. |
| DM3-G | Receipt verification | Exact destination, calldata, and matching `DealSettled` event are required; unrelated successful transactions are not settlement evidence. |
| DM3-H | Guarantee enforcement | Unsupported guarantees fail in both adapter selection and the contract; direct calls cannot bypass capability rules. |
| DM3-9 | Finality and receipt | **Superseded by M6 on 2026-10-01.** Confirmation counts are not finality; supported recovery now requires paired explicit `finalized` evidence (DM6-4, DM6-5). |

### M4 and M5 selections (no DM IDs)

M4 selected BN254 Groth16, Circom 2.2.3 / circomlib 2.0.5, Poseidon with numeric domain tags
2001-2009, BabyJubJub EdDSA role authorizations, reserved suite 2, depth four in the prototype, a
local Node/snarkjs prover, and a single-contributor setup discarded for public value. The circuit
shape fixes protocol version 1, suite 2, shielded mode, guarantees `0x7`, and zero fee. The
feasibility memo left vendor licensing (Railgun, Unlink, Kohaku, Tornado Nova), ceremony, and
Monad verifier gas as owner decisions. M5 selected the funded prototype direction (one supported
asset, depth-20 pool tree, encrypted local wallet, verified public cache) but published no
production artifacts; its gate remains open.

### M6 decisions DM6-1..DM6-12

| ID | Decision | Outcome |
|---|---|---|
| DM6-1 | What decides payment | Implemented conservative rule: a revision is unpaid only by `consumedDeals == false` at a final block past that revision's expiry; unknown consumption holds every reservation; the winner is found from the `DealSettled` log and verified calldata. **Owner decision and rationale are still blank.** |
| DM6-2 | Journal generalization | Decided: one shared `sdk/journal` generic over backend record types; Starknet layout unchanged. Legacy v1/v2 `accepted_at` timestamp provenance remains ambiguous; the owner chose compatibility on 2026-10-01, so a future explicit migration is still required. |
| DM6-3 | Mode order | Decided: public-bound completes the lifecycle and fault matrix first, then shielded on the same coordinator. |
| DM6-4 | EVM operation record and stages | Integrated: explicit nonce, local signing, persisted bytes, journaled broadcast, observation, reconciliation. On 2026-10-01 the owner removed the unjournaled backend submission APIs; supported submission uses the coordinator, and operator reconciliation requires paired finalized evidence. |
| DM6-5 | Finality and quorum | Implemented `finalized`-tag-only observation with canonical anchor rechecks; local tests proved self-consistent RPC histories can disagree. On 2026-10-01 the owner made two matching providers mandatory before accounting, wallet, or nonce changes; this is consistency, not consensus, and honest providers at different heights can hold recovery. |
| DM6-6 | Concurrency | Decided 2026-09-29: one unresolved operation per chain and gas-paying signer, shared across deployments; release only on a strictly greater finalized account nonce; different signers proceed independently. |
| DM6-7 | Policy ledger persistence | Decided: one reservation per signed revision, persisted with coordinator intent, committed for the winner, released only after the final winner or final expiry. **Open:** a cap on authorized expiry and whether the ledger scope includes Starknet `accept_and_settle`. |
| DM6-8 | Relayer | Decided: public-bound only (fee and recipient are signed terms; the shielded circuit fixes fee 0, so shielded is self-submitted). **Open:** how a buyer obtains a relayer quote and how a different relayer is chosen after the fee recipient is committed. |
| DM6-9 | Fault injection | Implemented durable-boundary fault hooks and a JSON-RPC proxy for dropped/falsified broadcasts, timeouts, and impossible anchors. Funded sweeps (56 public-bound, 68 shielded, later 75 crash points) pass locally; provider independence is not established. |
| DM6-10 | Scope exclusions | No Monad network evidence, shielded relayer compensation, disclosure package (M7), or consensus proof; only the two-provider consistency policy. |
| DM6-11 | Relayer recovery | Implemented `DurableRelayer` in relayer-owned directories; it holds no participant key, never signs an agreement, persists before sending, observes before retrying, and cannot clear a fence. The CLI is trusted-host newline-delimited JSON, not an authenticated remote multi-client gateway. |
| DM6-12 | Bounded historical observation | Implemented resumable public-bound observation with pinned anchors, bounded log/ancestry budgets, reorg suffix restart, and shielded bounded scans (1,000 blocks/call, 128 MiB cache). Pending is not evidence. Paired recovery can be held by provider lag. Cache compaction and fresh-install throughput remain limits. |

### M7 decisions DM7-1..DM7-4

| ID | Decision | Outcome |
|---|---|---|
| DM7-1 | Direct suite-2 grant signing | The participant's suite-2 seed signs the grant locally; either role can issue, no delegation, no separate disclosure signer, no circuit or hosted prover. The recipient uses a separate X25519 key. |
| DM7-2 | Versioned grant format | Version 1 keeps suite-1 keccak/secp256k1 and cannot authenticate suite 2; version 2 is exclusively suite-2 with version, issuer, recipient, deal id, expiry, Noise handshake, frame count, frames, and a 96-byte signature. Unknown versions fail; no fallback. |
| DM7-3 | Disclosure signature domain | Version-2 prologue hashes domain `EREBUS_DEAL_DISCLOSURE_V2_SUITE2` with issuer, recipient, deal id, and expiry; the signature is `Poseidon([3001, high128, low128])`. Tag 3001 is separate from payment tags 2004/2005; tests reject disclosure signatures as payment authorizations. |
| DM7-4 | Observation and metadata boundary | The public header exposes issuer key, recipient key, deal id, expiry, and ciphertext length; observers can link a reused issuer key. Only the recipient decrypts the transcript and opening. Payment is verified through the auditor's own configured deployment and paired RPCs; provider agreement does not prove provider independence; payment verification does not verify delivery. |

### M8 decisions DM8-1..DM8-15

| ID | Decision | Outcome |
|---|---|---|
| DM8-1 | Buyer identity for access | The agreement key is the access recipient (`access_recipient == buyer_authorization_key`); no delegated access keys. Requests bind domain, service, commitment, nullifier, resource digest, nonce, and expiry; ≤5-minute lifetime; replay during validity is possible; retrieval is idempotent. |
| DM8-2 | Payment and delivery separate | The seller verifies resource, buyer, quantity, and payload digest and requires finalized payment evidence before durable issuance; pending does not issue and does not prove nonpayment. `delivery_verified: false`; recovery needs seller cooperation, which cannot be forced. |
| DM8-3 | Shared artifacts, local witnesses | `erebus-artifacts` authenticates a trusted manifest digest, verifies lengths/hashes, locks, writes atomically, rejects symlinks, and repairs corrupt entries; the witness stays local and is never uploaded. Manifest authentication does not establish ceremony security or verifier code. |
| DM8-4 | Bounded disclosure observation | The public-bound disclosure CLI persists observation progress, accepts a deployment start block and bounded budgets, exits `2` for pending, and requires matching finalized evidence for exit `0`. Invalid grants fail before RPC. |
| DM8-5 | Buyer content verification | The buyer signs a short-lived retrieval request, verifies payload SHA-256, persists verified content, and can read it later from cache without a key or network. It reports `resource_verified: true` and `seller_reported_payment_finalized: true` but keeps `payment_verified: false` and `delivery_verified: false`. |
| DM8-6 | Freeze negotiation before authorizations | One frozen transcript; profile 1 carries draft terms with a zero root and private blinding; counters change only price/revision and reference the previous digest; both acceptances seal the root before signatures. Final authorizations stay outside the log. Recovery is idempotent and fails closed on corrupt freeze records. |
| DM8-7 | Bind discovery identity to shielded identity | A suite-1-signed binding (`EREBUS_AGREEMENT_KEY_BINDING_V1`) attests the descriptor digest, deployment domain, suite-2 key, and validity window inside Noise; no shielded key enters the public descriptor; bindings do not enter the frozen root and are not payment consent. |
| DM8-8 | Native payment uses durable consent | The operator fixes mode, deployment pins, keys, asset, gas account, fee caps, and the shared signer journal; the runtime pin must come from an independently verified immutable deployment, with code at the first block and no code at the preceding block; any recorded broadcast attempt disables automatic submission; later calls observe with two RPCs and separate checkpoints. A transaction fenced before broadcast can stall and needs operator recovery. |
| DM8-9 | Separate Metropolis registry | Metropolis packages publish to a separate index, not the stable Starknet channel; the builder stages `.devN` packages without editing source metadata and bundles native binaries in a host-tagged wheel; publication requires owner approval. |
| DM8-10 | Public-bound product first | Finish the whole public-bound loop (negotiation, settlement, delivery, packaging, self-hosting, x402) before deploying a shielded testnet pool; shielded driver evidence stays local until then. |
| DM8-11 | x402 `exact` | The x402 scheme is `exact`, for now. |
| DM8-12 | Buyer evidence directory | Combined MCP mode enables `retrieve_service_access` when `EREBUS_ACCESS_SERVICE_URL` is set and fixes the evidence directory to `<state_root>/agent`; a different `EREBUS_ACCESS_EVIDENCE_DIR` is rejected. |
| DM8-13 | x402 `exact` over Permit2 | Separate `x402-exact` mode through canonical `x402ExactPermit2Proxy`, seller-facilitated; the Permit2 nonce is the deal nullifier, so `(buyer, deal)` can pay once and cross-rail double payment is prevented by binding the agreement domain to the exact proxy. **Binding implementation choice: owner review pending.** Exposure is listed in the claims boundary below. |
| DM8-14 | Paired observation uses one shared finalized snapshot | Owner review pending (source fix, 2026-10-04): honest providers may report different current tips and finalized heights while agreeing on canonical history; requiring identical tips stalled the live scan. Paired observation derives one shared snapshot (the lower finalized and lower head of the two providers) and requires identical canonical blocks at those heights. Runtime pins, canonical hashes, complete history, and nonce agreement are unchanged; a falsified nonce, a finalized-ahead-of-head answer, or a different canonical hash still fails closed. The harness also bounds polling and requires the agreement lifetime to cover the verification window plus settlement/delivery. Released in `0.3.0.dev5`. |
| DM8-15 | Bounded observation issues contiguous ranges concurrently | Owner review pending (source fix, 2026-10-05): buyer, seller, and auditor each paid a sequential deployment-to-head scan bounded by the public RPC's 100-block `eth_getLogs` cap. `ObservationLimits` gained `max_concurrent_queries` (1..=32, hard ceiling 32, default 8); one window of contiguous, non-overlapping ranges runs concurrently and the durable checkpoint advances only after every query in the window succeeds. The scan start is unchanged, no range is skipped, and a missing response is never absence evidence. Ancestry walks the same canonical blocks in bounded concurrent windows with the same parent-hash, timestamp, and anchor checks. A pending-history reply reports durable `broadcast_attempts` and `stage`; the harness resumes read-only catch-up and calls `settle` again only with zero attempts, and observes only after any attempt, ambiguous submission, or missing diagnostic. The Rust expiry rules, canonical anchors, runtime pins, exact payment binding, and the one-payment fence are unchanged. Measured on the Monad public RPC: 48 queries took 51.7 s sequentially and 6.4 s at concurrency 16, about 21 minutes of log-query time for the live range (authentication, ancestry, retries, seller verification, and auditing are separate). Released in `0.3.0.dev5`. |

**Open release decisions:** secure shielded artifacts, ceremony evidence, and authenticated verifier
deployment; hosting and publishing the Metropolis registry (channel decided in DM8-9), platform
coverage, and public artifact distribution; hosted-service resources, quotas, retention, and
incident ownership; the x402 scheme and the measured per-request/prepaid/batched comparison; mainnet
activation (the default remains a testnet developer release). The x402 prepaid and batched models
remain experiments unless selected.

## Claims boundary and exposure

- **Public-bound settlement exposes** amount, asset, payer, recipient, fee, timing, and the
  participating parties, plus the complete accepted canonical agreement, blinding, and both final
  signatures in calldata. It proves payment execution, not hidden amounts or identities. Only the
  offchain negotiation history stays confidential.
- **x402 `exact` exposes** buyer, `payTo`, amount, token, the deal nullifier, and the Permit2
  payment authorization signature. The Erebus agreement opening, blinding, and bilateral agreement
  signatures are not published by this rail, unlike `ErebusSettlement`. Generic third-party x402
  client interoperability is unverified; the HTTP profile requires an authenticated Erebus request
  and a matching x402 v2 header.
- **Shielded settlement hides** amount and recipient on chain but still reveals timing and the
  participating endpoints. **Shielded has not run on Monad.** Suite-2 evidence is local Anvil only,
  with known-entropy test keys, a single-contributor setup, and test-only proving artifacts that
  must never hold real value. Production setup, ceremony, artifact distribution, and verifier
  administration do not exist.
- **Payment is not delivery.** An encrypted grant alone does not prove payment; independent
  finalized chain evidence is required. Resource-hash verification does not prove independent
  payment or a delivery audit. Seller receipts and `PAYMENT-RESPONSE` are seller assertions.
  `paid_but_undelivered` remains representable.
- **Measurements are not guarantees.** Local Anvil timings, block timestamps, and diagnostics are
  not Monad inclusion or finality latency. Live stage timings are local monotonic observations, not
  latency promises. Prepaid and batched are experiments, not product rails.
- **Trust is not independence.** Two RPC URLs are not independent providers; two-of-two consistency
  is not Byzantine consensus. Team-operated rehearsals, including the live runs and the withheld-path
  audit re-verification, are not external acceptance. Hosted operation does not exist.
- **Metadata stays exposed.** Transport hides content, not endpoints, mailbox ids, sizes, timing, or
  the fact of communication (DM2-9). Grant headers expose issuer key, recipient key, deal id, and
  expiry; reused issuer keys are linkable (DM7-4).

## Submission gates (unchecked items)

Rubric: Product Quality and Completeness, Technical Excellence, Monad Integration, Track Fit and
Problem Relevance, and Innovation and Impact, each 20%. The supplied deadline was October 13, 2026,
23:59 ET. Keep the official rules document with the final submission; this record is not a rules
source.

- [ ] A demo video of at most three minutes shows the product operating and a Monad interaction.
- [ ] A third party runs the submitted path from the README, without team-held fixtures.
  The reproducible handoff is [External Acceptance](metropolis-external-acceptance.md);
  it has not been run by an outside operator yet.
- [ ] Every release and privacy claim matches reproducible evidence.
- [ ] Independent external auditor verification with participant state unavailable. A team-operated
  re-verification of the live x402 payment with the participant paths withheld verified
  `payment_verified: true` (2026-10-05); the auditor and operator were still the same team.
- [ ] An external third-party fresh-environment rehearsal. The `0.3.0.dev5` channel exists at
  `https://poulavbhowmick03.github.io/erebus-metropolis/simple/` and a team-operated public install
  was verified on macOS arm64 (2026-10-04); no third party has run it. Linux qualification is
  CI-only.
- [ ] A recorded Monad demo, linked from the README and submission. The recording-ready script
  with real captured output is [Demo Script](metropolis-demo.md); the video itself is pending.
- [ ] Hosted services. Render deployment files are prepared and tested only; paid provisioning is
  not authorized and no hosted service exists.
- [ ] Inclusion and finality observation timings on live Monad, not differences between block
  timestamps. Local monotonic stage timings exist for public-bound and x402.
- [ ] M5's gate and the complete installed shielded negotiate-pay-access workflow (DM8-10 defers
  shielded deployment until the public-bound product is complete).

Do not mark these complete based on a local test, a document, a package build, or a team-operated
run.

### Demo requirements carried from the demo plan

The demo establishes two agents negotiating a service price privately, authorizing the same
agreement, and executing a matching payment; an auditor receives evidence for exactly one deal;
the seller releases an API capability after final settlement (a seller-controlled action). Use one
service and one supported asset, deterministic agent policies first and an LLM only after the
protocol passes the same tests. The settlement chain and privacy mode are chosen before the channel
opens; negotiating the chain is out of scope. A public-bound settlement exercises the coordinator
and backend boundary but does **not** satisfy the shielded demo gate; if shown, record it as an
architecture milestone, not a passing stage.

Measurable stages: source map, settlement feasibility (substitution fails), agreement
specification (valid and invalid vectors agree), private coordination (one transcript root across
retry/restart), Monad integration (local proof, relayed submission, note discovery, receipt
verification, replay failure, lost-response recovery), disclosure (auditor verifies one deal,
cannot decrypt a neighbor), service demo, and regression/rehearsal. The recording plan is a
90-second sequence: policies and private negotiation; matching commitments in both participant
records; public transaction and observer output with explicit metadata; finalized settlement and
successful API request; one-deal auditor disclosure; replay refusal and denied adjacent-deal
access. Measure the actual end-to-end duration before recording, label any time skip, and do not
present chain inclusion time as proof-generation or full-deal latency. The observer must run
without participant keys, transcript files, or auditor grants.

Store sanitized evidence under a dated `docs/runs/` directory: source commit, contract addresses,
chain id, pool/verifier versions, dependency revisions, reproduction commands, deterministic
policies, public transaction hashes, receipts, block identifiers, finality rule, raw observer
inputs, separately measured negotiation/proof/submission/inclusion/finality/delivery durations,
negative results for replay/wrong-recipient/changed-amount/expired/mixed-proof cases,
crash-after-broadcast recovery, one-payment confirmation, and a synthetic disclosure package
labeled as intentionally disclosed. Never publish private keys, local state directories,
credentials, or real confidential transcripts. Prior Starknet work stays separate from new
Metropolis transport, agreement, EVM proof, and Monad evidence; the organizer message permitting
significant new development belongs with the submission records, and this status does not
independently verify rules, dates, prizes, or sponsor eligibility.

## Reproducing evidence

Commands are recorded here as they appeared in the historical records. Local commands run from the
repository root unless stated. Live commands require operator configuration, funded accounts, and
explicit authorization.

```sh
# M0/M1: Rust and agreement vectors
cd sdk/rs && cargo fmt --check && cargo test --all-targets && \
  cargo clippy --all-targets -- -D warnings && cargo build --bin erebus-cli
cd sdk/core && cargo test --all-targets --locked
python3 scripts/check_agreement_vectors.py

# M2: transport
cd sdk/transport && cargo fmt --check && \
  cargo clippy --all-targets -- -D warnings && cargo test --all-targets

# M3: contracts and EVM adapter
cd contracts/evm && forge build && forge test
cd sdk/evm && cargo test --locked -- --include-ignored

# M4/M5: circuits, funded prototype, coordinator and crash matrices
cd circuits/m5 && EREBUS_M5_NATIVE_PROOF=1 EREBUS_M6_COORDINATED=1 npm run prototype
# add EREBUS_M6_MATRIX=1 for the funded shielded crash sweep in release mode

# M6: local-chain, coordinator, and relayer checks
forge build --root contracts/evm
cargo test --manifest-path sdk/evm/Cargo.toml --locked --offline -- --ignored
cargo test --manifest-path sdk/coordinator/Cargo.toml --locked --release --test lifecycle every_discovered -- --ignored

# M7/M8: native negotiation, payment, delivery, and disclosure
cargo test --locked --manifest-path sdk/shielded/Cargo.toml --test negotiation_cli -- --include-ignored
cargo test --locked --manifest-path sdk/shielded/Cargo.toml --test negotiation_cli payment_driver -- --include-ignored
cargo test --locked --manifest-path sdk/shielded/Cargo.toml --test negotiation_cli shielded_driver -- --include-ignored
uv run --locked pytest agents/tests/test_metropolis_loop.py \
  mcp-server/tests/test_metropolis.py sdk/py/tests/test_metropolis_seam.py

# x402 service-model comparison (local Anvil, experimental models)
cd contracts/evm && forge build && cd ../../sdk/evm && \
  cargo test --test x402_service_models -- --ignored --nocapture

# M8 deployment identity and readiness
echo '{"chain_id":10143,"rpc_url":"https://testnet-rpc.monad.xyz","timeout_ms":10000}' \
  | erebus-network-check
python3 scripts/check-evm-deployment.py   # operator configuration and contract pins

# M8 registry build/install check (local unpublished index)
uv run --locked python scripts/build-metropolis-registry.py \
  --version 0.3.0.dev20261004 --profile release --build --output artifacts/metropolis-registry
uv run --locked python scripts/check-metropolis-install.py --registry artifacts/metropolis-registry
# add --rehearse-x402 for the installed x402 recovery/auditor rehearsal

# M8 live rehearsal (operator plan, explicit authorization, funded accounts)
python3 scripts/metropolis-monad-rehearsal.py init --plan plan.json --workdir DIR
python3 scripts/metropolis-monad-rehearsal.py preflight --workdir DIR
python3 scripts/metropolis-monad-rehearsal.py run --workdir DIR --authorize-live-transactions
```

Published install path: [metropolis-install.md](metropolis-install.md) §10; the same commands and
platform qualifiers are in the [README](../README.md). The package channel, rubric boundary, and
external-acceptance checklist are in the
[submission gates section](#submission-gates-unchecked-items) above and in
[metropolis-roadmap.md](metropolis-roadmap.md). Installed commands, operator configuration, and
recovery boundaries are in [metropolis-operations.md](metropolis-operations.md). Architecture,
threat model, and agreement limits stay in [metropolis-architecture.md](metropolis-architecture.md),
[metropolis-threat-model.md](metropolis-threat-model.md), and
[metropolis-agreement.md](metropolis-agreement.md). Record new stack friction in
[friction.md](friction.md); existing Metropolis friction entries referenced by this document
include F12, F38, F43-F46, F48-F50, and F57.

## 2026-10-13 Product Integration In Progress

The published `0.3.0.dev2026101202` remains separate from the moving candidate.
These source additions are not an installed release, an audit or outside acceptance.

- Sponsored shielded HTTP service and `erebus-shielded-relayer` binary implemented. Authentication, body limits, deadlines, persistent rate limits and lifetime gas-cap reservations are operator policy. Each instance has one configured authentication principal, not multi-tenant isolation.
- Native buyer, SDK and MCP sponsorship implemented. The buyer persists a destination-bound delegation before HTTP; remote status is diagnostic only. Its own paired pool evidence determines payment verification.
- Funded native buyer regression passed on local Anvil: unavailable service caused zero sends; retry used one sponsor transaction; removing relay configuration did not authorize direct signing; observation recovered finality without another send.
- Relay-focused release unit suite: 20 passed, including bounded owner resume, fee-only replacement history and replacement write-fault boundaries. A full integrated product rerun is still required.
- Wallet SDK and MCP action policy implemented. Agents cannot supply wallet paths, keys, arbitrary calldata or targets. Amounts and withdrawal recipients are bounded; replacement fee caps are operator-fixed. Deposit requires a caller reference and refuses older native commands without that capability.
- Native deposit reference journal implemented, with policy/note binding and fail-closed corruption/deletion tests. Wallet binary suite: 6 passed. Pending sync reports bounded next-block progress. Both funded installed-wallet regressions passed on the final funding-reference implementation (2 passed, 180.92 seconds). Each gate copies its wallet executable once so concurrent builds cannot change it between commands.
- Python SDK tests plus focused MCP/operator-generator suite: 205 passed. This does not replace installed two-agent qualification.
- Targeted release clippy passed with warnings denied for the library, buyer payment CLI, wallet CLI, relayer CLI, negotiation driver and wallet operations tests.
- Published wallet read-only seller catch-up advanced primary through 69933652 and peer through 69921652, then returned pending. The recipient note at the payment block is not yet recovered. No live write or new payment was made in that run.
- Approval renewal now has an explicit caller reference, while legacy approval identities remain supported. SDK/MCP requires references for approval, wrap, unwrap and deposit. Native funding references bind the original action and amount; the approval identity test and integrated funded wallet reruns pass.

- Explicit owner `--resume` resends only retained bytes, with at most 16 durable retries. Normal HTTP duplicates stay observation-only. `--replace` uses an optional owner-fixed fee ceiling configured before the first request; quotas reserve the ceiling in full. Changing the ceiling on retained storage fails closed. Replacement preserves sender, chain, nonce, native value, calldata and gas; old hashes remain observable and the actual successful winner is recorded.
- Full relay admission/funded release rerun: 7 passed, 0 failed (191.33 seconds). It covers dropped-send owner resume, replacement winning, older attempt winning, own-receipt recovery after restart, paired admission negatives and external proof verification. Earlier new-test runs exposed an undersized fixture quota and replacement re-admission after a send; the fixture budget now includes the ceiling, and a repeated replacement is observation-only. This final rerun passed on the corrected source.
- Focused wallet/SDK/MCP Python rerun: 65 passed. Release clippy with warnings denied passed for the relay library, binary and admission tests. Source binary version/help smoke checks passed without keys or RPC.
- Both RPC providers report zero genuine testnet USDC for the retained buyer address. MON funding does not supply USDC. A live USDC lifecycle still needs that asset; no fixture mint is substituted.
- Native wallet `status`, SDK and opt-in MCP `wallet_status` now report paired provider-finalized native/asset balances, pool allowance, retained operations and checkpoint block lag. All account reads use the same canonical block hash; deployment/runtime pins, pool identity, agreement between providers and post-read anchors are checked. These diagnostics do not authorize spending, reconcile state or verify a payment. Existing policies must explicitly allow `status`; older native capabilities are refused.
- Diagnostic verification: 2 controlled RPC tests passed (honest lag, conflicting/malformed values, missing code/getters, anchor mutation and finality regression), 7 wallet binary tests passed, and 7 RPC unit tests passed. Focused Python SDK/MCP rerun: 66 passed. Release clippy for the library, wallet binary and diagnostic/funded wallet tests passed with warnings denied.
- Funded installed-command wallet regressions passed together: 2 passed, 200.81 seconds (ERC-20 approval/deposit/retry/withdrawal plus WMON wrap/deposit/withdraw/unwrap). The diagnostic fixture waits for finalized mint/approval state; it does not substitute the newer head balance. An earlier test build raced the import edit, and the first finalized-balance assertion preceded mint finality; both were corrected before this rerun.
- Live read-only source diagnostics passed at finalized block 70490161. The seller has 2 MON for gas, zero external WMON and zero pool allowance. Primary/peer wallet checkpoints are 556509/568509 blocks behind that anchor; the retained recipient note is still not included. This exposes a substantial cold-history catch-up bottleneck, not a completed seller lifecycle. No live transaction was sent.

Open work includes installed service/operator recovery qualification, full live asset lifecycles, remaining wallet diagnostics/agent ergonomics, and a stable packaging handoff. Independent setup, audit and outside acceptance remain external gates.
