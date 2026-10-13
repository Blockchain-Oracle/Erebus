# Metropolis Leakage Review

Date: 2026-10-13. Scope: team-run source review and focused local tests.
This is not an audit, live observer report or independent acceptance.

## Reviewed Identity

HEAD: `482d912cbc33e5f96de85f49b3006d631454b607`, with retained dirty inputs.
Production input identity:
`f6ec6bcc27f39bc39155bf439301a80ec6e0e4361c5083c12fbe6f6db4efb6dd`.
The registry identifier reproduced that identity before the final documentation edits.
Concurrent onboarding edits later changed the combined identity to
`1526fdbb35347ef0cd60ba84f17e4dcb28a56dcd3e77f988122eae77d5954333`.
The tested baseline onboarding file hash was
`c2f6a48d24b86efa8df41ab4c43b8d8d1cdec630a5c787822a8b52f7ee2ae20b`.
This report does not qualify those newer packaging inputs or a new release.
Rust wallet, RPC, operations and reviewed SDK source hashes remained unchanged at that check.

## Findings

No critical or high-severity secret leak was demonstrated in the reviewed paths.
This conclusion applies only to the source and tests listed below.

1. **Low: onboarding echoes native errors.**
   `operator_init.command` copies the native reply's `error` into a `ValueError`.
   The top-level handler prints that message. A mocked error containing
   `LEAKAGE_REVIEW_SENTINEL` reproduced `native_error_echoed=True`.
   Current reviewed native errors are fixed strings. No actual secret leak was shown.
   Claude should replace arbitrary native text with a fixed diagnostic or safe code.
   Sources: `mcp-server/src/erebus_mcp/metropolis_setup/operator_init.py:153` and `:962`.
   A compromised installed binary can already read local secrets; this is defense in depth.

2. **Documentation boundary: wallet output is not public-chain data.**
   Native `note_view` returns amount, commitment, nullifier and reservation status.
   SDK/MCP wallet tools deliberately return these fields to the authorized agent.
   An unspent nullifier can later correlate an agent transcript with an on-chain spend.
   The threat model now distinguishes operator/agent visibility from public visibility.
   Sources: `sdk/shielded/src/bin/erebus_wallet.rs:658`;
   `sdk/py/src/erebus/_metropolis_wallet.py:154`; `mcp-server/src/erebus_mcp/metropolis.py:134`.

3. **Test limitation: fixture markers are not a general secrecy proof.**
   The observer test rejects specific fixture secrets in calldata and verifies the sponsor sender.
   It cannot prove anonymity, absence of encoded leakage or safe external logging.
   Source: `sdk/shielded/tests/relay_adversarial.rs:679`.

## Visibility By Surface

| Surface | Visible | Not sent by the reviewed path |
|---|---|---|
| Private transfer calldata | Proof; chain, pool, version, asset; deal commitment/nullifier; root; input nullifier; output commitments; expiry | Plain amount, participant authorization keys, spend secrets, note salt, witness |
| Public transaction | Gas sender, target, timing, fees, calldata and receipt | A stealth address or guaranteed unlinkability is not provided |
| Deposit/withdrawal | Funding accounts and amounts; withdrawal recipient/amount; token and pool events | These boundaries do not conceal amounts |
| Sponsored request | Version and exact public calldata; Bearer credential in the authenticated HTTP header | Agreement opening, witness or participant keys in the JSON body |
| Relay status | Derived operation identifier, retained transaction hash and finalized block outcome | Token, raw signed transaction, participant opening |
| Wallet CLI/SDK/MCP | Exact note amounts, asset, commitments/nullifiers, states and account diagnostics | Spend secret, salt, witness or key file contents |
| Payment SDK/MCP | Deal identifiers, shortfalls, progress, hashes and timings | Unexpected fields and arbitrary subprocess errors are rejected |
| Disclosure | Authorized recipient decrypts selected messages, agreement opening and authorizations | CLI summary does not return plaintext terms or spending material |

Sources: `ErebusShieldedPool.sol:131-180`, `transfer.circom:249-251`,
`relay_request.rs:22-68`, `relay_http.rs:304-410`, `relay_client.rs:86-168`,
`sdk/evm/src/disclosure/cli.rs:208-231,627-642`.

## Transport And Logging

The buyer client requires HTTPS or an explicit loopback HTTP exception.
It refuses redirects and returns fixed diagnostics for failed or malformed replies.
The service binds loopback; remote deployment requires an authenticated TLS gateway.
The gateway can see credentials, proof calldata, client IP and request timing.
The reviewed code does not establish a gateway log-retention or redaction policy.
Do not log Authorization headers, request bodies, selected evidence or wallet tool transcripts.
Local journals retain signed bytes and public proof calldata; they are not safe support attachments.
No external LLM provider, proxy, crash collector or production telemetry was tested.

## Executed Checks

- Focused wallet/payment SDK and MCP tests: **66 passed**, 8.32 seconds.
- Disclosure MCP and operator-init tests: **60 passed**, 25.01 seconds.
- Shielded `--lib relay_` release suite: **20 passed**, 2.47 seconds after build.
- Sponsor/public-calldata observer release test: **1 passed**, 1.49 seconds.
- Core normal/pretty Debug redaction test: **1 passed**.
- Mocked onboarding error sentinel: **echo reproduced**; no keys or RPC used.

Exact commands:

```sh
uv run --frozen pytest sdk/py/tests/test_metropolis_wallet.py sdk/py/tests/test_metropolis_seam.py mcp-server/tests/test_metropolis.py -q
uv run --frozen pytest mcp-server/tests/test_disclosure.py scripts/tests/test_metropolis_operator_init.py -q
cargo test --release --locked --manifest-path sdk/shielded/Cargo.toml --lib relay_ -- --test-threads=1
cargo test --release --locked --manifest-path sdk/shielded/Cargo.toml --test relay_adversarial sponsor_is_the_public_sender_and_public_calldata_carries_no_participant_keys -- --ignored --exact --test-threads=1
cargo test --release --locked --manifest-path sdk/core/Cargo.toml --test shielded_vectors private_agreement_objects_redact_normal_and_pretty_debug -- --exact
```
The observer used local controlled RPC fixtures, not Monad.
No live operator directory was read and no live transaction was sent.

## Remaining Gates

Claude owns the onboarding hardening finding and installed agent qualification.
An actual installed live observer report must verify the public sender and asset effects.
Review deployed gateway logs, external model telemetry and support bundles before claiming operational secrecy.
Independent audit and outside acceptance remain open.
