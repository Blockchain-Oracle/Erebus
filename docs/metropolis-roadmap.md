# Metropolis Roadmap

Updated: 2026-10-12. Branch: `metropolis`. Submission target: October 13, 2026.

This file tracks tasks and owners. Detailed runs, decisions and hashes belong in
[status](metropolis-status.md), [operations](metropolis-operations.md), and [install](metropolis-install.md).
[Agreement](metropolis-agreement.md) remains normative for protocol formats.

Checked items have evidence at the stated scope. Local tests, team-run testnet workflows
and published installed workflows are separate gates. Agent reports are not independent audits.

## Product Target

Install Erebus, discover a service, negotiate privately, pay from shielded notes,
recover interruptions, retrieve the resource, and disclose one deal to an auditor.
Support qualified ERC-20 assets and MON through WMON. SDK, CLI and MCP are the product surface.

Current shielded deployment: test-only, single-operator setup, unaudited, Monad testnet only.
Monad Private Settlement is a possible future backend, not a dependency.
A frontend and paid hosting are not required for the self-hosted testnet loop.

## Current Baseline

| Milestone | Current state |
|---|---|
| M0-M2 | Baseline, agreement core, encrypted transport and authenticated discovery implemented and tested locally. |
| M3 | Public-bound settlement runs on Monad testnet; payments are public. |
| M4 | Agreement/payment proof relation implemented; research gate complete, not an audit. |
| M5A | C1-C4 implemented and tested; clean freeze remains open. |
| M5B | Test-only setup/bundles work; independent ceremony, HTTPS hosting and final qualification remain open. |
| M5C | Wallet/assets/access pass local gates; live completion and sponsorship remain open. |
| M5D | Adversarial tests pass; final integrated rerun and independent audit remain open. |
| M6-M7 | Recovery/disclosure pass local gates; live recipient-only shielded auditing now passes in a separate team process. |
| M8S | WMON/USDC pools deployed; one WMON private payment finalized. Full installed live workflow remains open. |
| M8O | Public dev5 published; private candidate packages remain unpublished and release-ineligible. |
| M9 | Outside acceptance and operating video remain open. |

### Live Evidence

- [x] Deploy and authenticate Poseidon, three verifiers and WMON/USDC pools.
- [x] Wrap MON, approve the WMON pool and finalize a proving deposit.
- [x] Finalize one private WMON payment with one broadcast attempt and committed accounting.
- [x] Verify that payment through the product's finalized-evidence path.

| Rail | Transaction |
|---|---|
| Shielded WMON | `0xe01666d8c4e16efe8195da8047674c71a325ee20ff250f02f4fdf570a04934fa` |
| Public-bound | `0x1ba9ddb363a0c916f8e41ca8eca7d4c524d1ba4722bf4ea37ed423547700ff73` |
| Public x402 exact | `0x8a0272e69188ac3be52783ddfc4d4436496a58f2d5c63d808cd3320a1fa5df43` |

The shielded run mixed installed negotiation with frozen source-built payment commands and warm history caches.
It is team-run evidence, not a published installed-only workflow.
Live resource retrieval after seller restart and recipient-only auditing now pass with source-built commands.
USDC payment, withdrawal/unwrap and the final installed-only workflow remain open.

## Ownership

| Owner | Work |
|---|---|
| Codex | Production Rust, circuits/contracts, deployment integration, roadmap/status and approved live runs. |
| OpenCode | Funded regression tests and reproducible failures; no production edits. |
| Claude Code | Packages, registry/install scripts, workflow, native qualification and install guide. |
| Poulav | Funding/release decisions, contributor/auditor arrangements, outside tester, video and submission. |

Preserve the dirty candidate and open merge. Do not reset, stash, abort, switch branches or touch `main`.
Use the applicable authorization before committing, pushing, signing, publishing, deploying or spending.
Record source/binary identities and rerun affected gates after changes. Keep secrets and witnesses out of reports.
Maintain the 5 GiB disk guard. Emulated Linux evidence does not count as native Linux qualification.

## Deadline Execution Order

1. Codex: finish retained wallet recovery, withdrawal/unwrap, repeat spending and the USDC workflow. Never resubmit the finalized deal.
2. Claude: qualify the fixed withdrawal candidate while Codex runs frozen live binaries. Preserve older package evidence.
3. OpenCode: test receipt attribution, interrupted reservation cleanup and paired-recovery failures. Report reproducible defects immediately.
4. Codex/Claude: freeze a clean revision, sign/host both pool manifests and run native macOS/Linux qualification.
5. Codex: complete the installed live loop. Claude: publish the approved packages and verify fresh public installation.
6. Poulav: arrange an outside operator and record the operating video while release work finishes.
7. Cross-check the submission against actual receipts, package identities and documented privacy limits.

The current withdrawal candidate is `e4e4f8323ba40e34ac4701d04d52d55f1ca5182328d9b2bf81a75fcc8f495130`.
It passes local receipt-attribution and wallet gates but remains dirty, test-only and unaudited.
Claude's r7 local macOS qualification passed. Its release hash and all 22 indexed evidence files were checked.
It is not yet a published, real-HTTPS or native-CI-qualified release.

M5R remains a separate workstream. Integrate it only with time to rerun affected gates; otherwise submit the self-submitted boundary explicitly.
Independent ceremony, audit and outside acceptance require actual participants. The deadline does not waive these gates.

## Remaining Work in Dependency Order

### M5A. Freeze and Review the Shielded Design

- [x] Trace circuits, pool, witness mapping, wallet, recovery and disclosure invariants.
- [x] Implement strict EdDSA acceptance matching Rust, including subgroup checks (C1).
- [x] Bind note commitments/nullifiers to chain, pool and verifier version (C2).
- [x] Constrain change ownership and retain encrypted spending material durably (C3).
- [x] Reject equal agreement keys in the SDK; disclose pool-level self-deal allowance (C4).
- [x] Preserve agreement encoding and frozen negotiation before authorizations.
- [x] Document gas-payer exposure and the current zero-fee submission policy.
- [ ] Close the merge and freeze a clean approved candidate with public source hashes.
- [ ] Resolve remaining security findings against the frozen revision.

Completion gate: approved design, pinned implementation and explicit findings record. This is not an audit.

### M5B. Artifact Setup and Distribution

- [x] Verify the selected phase-1 transcript and automated phase-2 test chains.
- [x] Build test-only keys, matching verifiers and complete source/artifact inventories.
- [x] Generate WMON/USDC pool-bound manifests and verify their verifier identities.
- [x] Require pinned publishers and authenticated manifests, not hashes alone.
- [x] Test reuse, interruption, tamper, wrong domain/version/publisher and disk failures.
- [x] Prove all three transitions from installed macOS packages for both deployed pool domains.
- [x] Record earlier Linux prototype proving evidence with its native/emulated scope.
- [x] Prepare HTTPS hosting checks and native dual-platform qualification workflows.
- [ ] Receive independent phase-2 contributions and separate verification/destruction attestations.
- [ ] Produce independently contributed final keys, verifiers and matching manifests.
- [ ] Sign, upload and verify both approved pool manifests/artifacts over real HTTPS.
- [ ] Qualify final hosted inputs on native macOS arm64 and Linux x86_64.

Completion gate: installed proofs use authenticated, qualified artifacts. Prototype distribution does not close independent setup.
Sagar/Sudipta are designated contributors and Ishita the verifier; independent participant evidence remains absent.

### M5C. Complete Asset, Wallet, and Service Integration

- [x] Implement fenced approve/deposit/withdraw/wrap/unwrap/sync commands.
- [x] Bind signed transactions to chain, sender, nonce, target, value and calldata.
- [x] Require exact receipt/event evidence and match withdrawal consumption to its own finalized successful attempt.
- [x] Persist attempts before sending and retain reverted/replaced transaction history.
- [x] Implement explicit same-nonce, same-intent fee replacement and observation-only recovery.
- [x] Complete installed local ERC-20 and wrapped-native workflows with random operators.
- [x] Recover recipient/change notes and reuse them in further private deals locally.
- [x] Test stale backups, index corruption, endpoint changes and actual local withdrawal rollback.
- [x] Run chain-level durable-write sweeps across wallet operation kinds.
- [x] Implement buyer-bound snapshot/metered access with durable issuance and restart recovery.
- [x] Keep payment, resource hash, issuance and delivery flags separate.
- [x] Test paired snapshots/read retries without lost progress, revived notes or extra sends.
- [x] Add bounded longer retrieval requests and authenticated auditor pending/progress handling locally.
- [x] Certify an indexed finalized payment without scanning unrelated later blocks; retain full scans for absence and wallet recovery.
- [ ] Qualify WMON/USDC behavior, issuer/admin risks and exact accounting on Monad.
- [x] Complete live paid-resource retrieval after seller restart and recipient-only auditor verification, using source-built commands.
- [ ] Complete live recovery, repeat spending, withdrawal and WMON unwrap.
- [ ] Integrate sponsorship with the durable payment fence (M5R).

Completion gate: installed buyer/seller/auditor flows pass locally. Live assets and sponsorship have separate gates.

### M5R. Shielded Relayer and Submission Privacy

Proposed first scope: an off-chain, operator-sponsored service using the existing pool.
The relayer signs the outer transaction with its own gas account.
Forwarding buyer-signed bytes does not hide the buyer.
No deliberate batching delay, extra settlement contract or user-paid fee is assumed.
Implementation details and deployment/spending need the applicable owner approval.

- [ ] Specify requests, authentication, limits, sponsorship budget and idempotency identity.
- [ ] Accept only proofs/public inputs; reject witnesses, spending keys and agreement openings.
- [ ] Authenticate pool/domain/runtime and verify the proof before allocating sponsored gas.
- [ ] Persist intent, nonce claim, signed bytes and attempts before broadcast.
- [ ] Return durable status/hash and resolve lost responses through finalized evidence.
- [ ] Bind buyer recovery to the relayed operation; never recharge on ambiguous results.
- [ ] Test concurrent requests, copied proofs, restart, exhausted funds, replacement and provider disagreement.
- [ ] Verify the public sender is the relayer, with no buyer/seller key in transfer calldata.
- [ ] Test redaction and document relayer IP/timing visibility and censorship risks.
- [ ] Measure direct versus relayed submission, inclusion and finality on the same candidate.
- [ ] Expose explicit sponsorship in CLI/SDK/MCP; prohibit silent direct-submission fallback.
- [ ] Rehearse self-hosting and alternative relayers without new payment authorization.
- [ ] Run an approved live relayed payment and archive its public observer report.
- [ ] Include the relayer in audit scope and final installed qualification.

Completion gate: one relayed payment finalizes, recovery never pays twice, and the relayer is the public sender.
This hides direct submission identity only. Funding, withdrawals and small anonymity sets remain linkable.
Relayer compensation requires a separate reviewed authorization design; never add unconstrained fees.
Stealth withdrawals and timing/batching defenses are later work, not current guarantees.

### M5D. Adversarial Testing and Independent Audit

- [x] Test identity expiry/clock failures and strict Rust/circuit signature acceptance.
- [x] Run witness mutations, differential vectors, proof/public-input mutations and replay negatives.
- [x] Test exact contract expiry and deposit/withdraw callback rollback.
- [x] Run real-proof conservation sequences and independent tree/wrapper invariants.
- [x] Run funded recovery, provider conflict/lag and interrupted-history regressions locally.
- [x] Test bounded read retries without lost progress, revived notes or extra sends.
- [x] Inspect proof-only RPC/calldata/log shapes and neighboring-deal isolation.
- [x] Harden journal paths, artifact requests, relay/indexer inputs and model-visible diagnostics.
- [ ] Rerun affected suites on the frozen candidate, including legacy regressions.
- [ ] Finish cross-surface leakage review, including M5R and live observer evidence.
- [ ] Commission an independent circuit/contract/wallet/service/setup audit.
- [ ] Publish exact scope/revision, report, findings and limitations.
- [ ] Fix findings and obtain reviewer retesting; leave no unresolved critical/high findings.

Completion gate: the frozen candidate passes tests and independent review/retesting. AI review and CI are not an independent audit.

### M8S. Monad Shielded Testnet Qualification

- [x] Obtain testnet deployment/run authorization and fund dedicated accounts.
- [x] Deploy/authenticate WMON/USDC pools, Poseidon and verifiers through paired finalized reads.
- [x] Negotiate a suite-2 agreement with separate installed participant commands.
- [x] Wrap, approve, deposit and settle one private WMON payment using product commands.
- [x] Verify finalized payment, one broadcast attempt and committed accounting.
- [ ] Run all commands from one qualified installed release with participant key isolation.
- [ ] Complete USDC payment and both assets' recovery/repeat-spend/withdrawal workflows.
- [ ] Complete WMON unwrap with exact finalized accounting.
- [x] Retrieve the paid resource after seller restart without another payment, using source-built commands.
- [x] Verify a grant with participant paths withheld and authenticated runtime pins in a separate team-operated auditor process.
- [ ] Run live prepaid access, lost-response recovery and policy denials.
- [ ] Archive public calldata/log exposure and metadata-correlation limits.
- [ ] Measure install, download, negotiation, proof, submission, inclusion, finality, recovery and delivery separately.
- [ ] Publish a clean-source, native dual-platform-qualified private prerelease after approval.

Completion gate: both approved assets complete reproducible installed live flows. Deployment or one payment alone does not pass.

### M8O. Operations and Developer Release

- [x] Publish public-bound/x402 dev5 and retain dev4 in the cumulative registry.
- [x] Prepare private onboarding, allowance/funding diagnostics and self-hosting tools.
- [x] Qualify r5 packages on macOS with both pool domains and local funded rehearsals.
- [x] Archive r5 after source changes and retain its candidate/artifact identities.
- [x] Qualify r6 locally on macOS for both pools; preserve its superseded source identity and reports.
- [x] Qualify r7 locally on macOS with the withdrawal fix, both pool domains and installed funded rehearsals.
- [ ] Qualify the next candidate on native macOS/Linux with real hosted inputs.
- [ ] Rehearse final-release persistence, backups, endpoints, limits, health and secret-free diagnostics.
- [ ] Update guides/examples to the qualified revision and require all five disclosure runtime pins.
- [ ] Verify release rollback/withdrawal, notices, platform limits and registry retention.
- [ ] Publish approved packages and verify fresh installation outside the checkout.
- [ ] Deploy promised hosted services only with separate account/budget authorization.

Completion gate: public docs/packages support the declared workflow without unpublished team helpers.
Render remains preparation-only. Hosted services are optional unless the release promises them.

### M9. Independent Acceptance and Submission

- [x] Prepare the outside acceptance handoff and operating-demo script.
- [ ] Run the private workflow with an outside operator using only public docs/packages.
- [ ] Record both assets, local proving, recovery, access, disclosure and withdrawal/unwrap.
- [ ] Record and host an operating video with a real Monad interaction.
- [ ] Verify portal rubric, mandatory artifacts, deadline/timezone and applicable bounty conditions.
- [ ] Separate Starknet work, Metropolis work, AI assistance and prototype limitations.
- [ ] Cross-check claims against exact revisions, manifests, test reports and live receipts.

Completion gate: outside acceptance and mandatory submission artifacts exist. Team rehearsals and scripts do not substitute.

## Privacy and Asset Boundary

Wrapping is asset conversion, not privacy. Shielded proofs hide inner-payment amounts and spending keys.
Observers still see gas senders, pool/asset, expiry, commitments, nullifiers and timing.
Deposits/withdrawals expose addresses and amounts. A small pool does not establish relationship anonymity.

Current private transfers are self-submitted. No stealth-address or shielded-relayer guarantee is implemented.
Public-bound and standard x402 payments remain public. Never silently downgrade a shielded request.
Keep keys and proving local. Artifact/indexer services introduce metadata and availability dependencies.

Each pool supports one immutable ERC-20. MON uses `MON -> WMON -> shielded notes -> WMON -> MON`.
USDC has issuer/admin powers. Arbitrary ERC-20, ETH/WETH, direct native-MON pools and mainnet assets are not qualified.
Record transfer behavior, decimals, runtime identity, backing and admin risks for each supported asset.

## Mainnet and Later Work

- [ ] Complete independent setup, audit/retesting and outside acceptance for the launch scope.
- [ ] Qualify mainnet assets, wrappers, gas exposure and deployment identities.
- [ ] Publish monitoring, incident response, recovery and upgrade/immutability policies.
- [ ] Obtain separate mainnet launch and funding authorization.

Later: stealth withdrawal destinations, optional batching/timing defenses, additional qualified assets,
other EVM deployments and confidential-domain adapters. These are not current testnet guarantees.
