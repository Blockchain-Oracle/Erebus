# Integrated Release Readiness

Review date: 2026-10-13. Decision: **not ready for the next release**.
This is a source/evidence review, not native qualification or publication approval.
The published `0.3.0.dev2026101202` remains separate and unchanged.

## Integration Progress

The relayer inventory is now integrated. New publication checks require
identity-bound generated-workflow and installed-relayer reports.
Focused onboarding/publication/registry tests passed **126**, with no failures.
The installed relayer from Claude's local `0.3.0.dev2026101305` passed all five
funded loopback chain-10143 tests in **91.73 seconds**. Its hash matched the release manifest.
The fixture chain/loopback refusal test also passed, and Rust format checking passed.

These results close the earlier missing fixture/inventory implementation gaps locally.
They do not qualify the newer onboarding package or native CI.
The binary fixture covers buyer transport, not the complete payment-CLI delegation lifecycle.
The workflow currently does not generate/upload the two new reports required by the publication checker.
Claude must finish that native wiring and the installed separate-agent gate.
Disk fell below the 5 GiB guard as the funded run completed; no further builds were started.
No caches were deleted and no other agent's process was stopped.

The workflow harness's private-journal prerequisite now has a public read-only
replacement: `circuits/m5/scripts/export-testnet-replay.mjs`.
Fourteen offline tests passed, and paired real RPC reads verified all six creation
transactions. Exported files contain already-public signed bytes, not private keys.
Claude must wire this exporter into native qualification; that invocation has not run in CI.
The exporter/generated-agent invocation is now wired for hosted native qualification.
Platform comparison invokes the same identity-bound feature checks as publication.
Seventy-seven focused tests pass, and workflow YAML parses. Actual native execution
and the relayer-report adapter/invocation remain open.

The local generated-agent report for candidate `af7e69d8` was inspected directly.
Both mock-asset pools record one recovered settlement, verified resource hash and
auditor payment verification, with 12 total operator broadcasts. Its rehearsal pins
are correctly excluded from publication evidence. This does not qualify a native release.

## Evidence Identities

- Reviewed rehearsal: HEAD `482d912`, dirty source `f6ec6bcc27f39bc39155bf439301a80ec6e0e4361c5083c12fbe6f6db4efb6dd`.
- Claude's local package: `0.3.0.dev2026101305`; release manifest `96088431211dd1236a1d5255decd29354ad3ecb12bbb26a2d903f8f6b5f2d374`.
- Live-tree observation during this review: `b6feb9cbc5101f0db7147f97283fca29a5e975fc2039b42d4db08c42120c3d43`, 205 input files, release-ineligible.
- Concurrent onboarding changes are ongoing. Earlier package results do not qualify this newer identity.

Claude's report records both-pool HTTPS proving, two asset rehearsals, metered and x402 passes.
It records candidate onboarding refusal and an unexecuted funded installed-relayer gate.
Relayer health/authentication/restart checks without sends do not close that gate.

## Blocking Findings

1. **New release gate execution is incomplete.**
   `publish-metropolis-release.py:105-172` requires proving and older installed rehearsals.
   The original checker did not require the new generated-agent workflow or funded installed-relayer report.
   Those guards and refusal tests are now implemented; native execution/report wiring remains open.
   `installed_onboarding` only proves native key/binding preparation, not `erebus-init` and its generated workflow.
   Claude must add identity-bound reports and refusal tests before the new feature scope can publish.
   This finding does not invalidate the older release's declared scope.

2. **Relayer inventory gap is resolved locally.**
   `scripts/build-metropolis-registry.py:25` now includes the new service binary.
   The registry inventory tripwire passes. Final native inventory/license qualification remains open.

3. **The next candidate is not qualified.**
   Onboarding rehearsal support changes package inputs. It needs a new exact identity and affected reruns.
   OpenCode must supply the loopback chain-10143 fixture for the actual installed service.
   Do not weaken the production chain restriction or count an in-process service as installed evidence.

4. **Clean revision and version authorization are missing.**
   The working tree is dirty. Eight existing local commits precede the new production changes.
   Current authorization names only `0.3.0.dev2026101202`, not any newer version.
   Qualification must run with `publish=false`; publication requires the next exact authorized version.

## Proposed Commit Scope

These are review groups, not staging instructions or approval. The owner supplies commit messages.

| Group | Include after review | Keep separate |
|---|---|---|
| Existing local stack | Eight commits `ab6dc3b` through `482d912`; onboarding/packaging | Do not autosquash or rewrite previously pushed history |
| Production integration | Shielded relay modules/binary, wallet/deposit references, payment/RPC/operations, SDK/MCP, coordinator delegation and journal updates | Cold-history changes from the separate worktree until tested |
| Supporting tests | Relay/admission/fencing/capability/diagnostics, wallet faults/operations, paired harness and SDK/MCP regressions | Ignored build outputs, keys, live stores and raw signed bytes |
| Circuit CI fixes | M4 historical vector fixture/prototype/README; M5 portable bundle test; related CI change | No circuit, key or verifier regeneration is implied |
| Packaging integration | Relayer inventory and licenses; rehearsal onboarding; full workflow harness/reports; publication guards | Existing release authorization must not be reused for a new version |
| Evidence/docs | README, roadmap/status/threat model and leakage/readiness reports | Based House briefs are unrelated and need separate review |

No blanket `git add .`. Review every tracked and untracked path from an isolated candidate.
No commit, push, main integration or publication was performed in this review.
Local `origin/*` refs were inspected; no fetch was run, so remote freshness is not claimed.

## Native Qualification Preparation

The old `native-ci-dispatch-inputs.json` is historical. It incorrectly lists publisher,
signatures and hosted artifacts as missing. Preserve it as evidence; generate new inputs.
The recorded publisher is `0xcd71b49f350f536ee4c4a27c8d0104be91f684fe`.
Both signed hosting manifest pins remain in `packaging/metropolis/testnet-release.json`.

After the blockers close, generate a new inputs-only JSON with the approved version,
release profile, `publish:false`, both hosted domains, publisher and deployment record.
Do not copy the old wrapper object as workflow inputs.

```sh
# Preconditions: reviewed clean commit pushed to this branch/tag; owner-approved version.
gh workflow run metropolis-registry.yml --repo PoulavBhowmick03/Erebus \
  --ref "$QUALIFICATION_REF" --json < "$INPUTS_JSON"
gh run view "$RUN_ID" --repo PoulavBhowmick03/Erebus --json headSha,conclusion,jobs
gh run download "$RUN_ID" --repo PoulavBhowmick03/Erebus --dir "$ARTIFACTS_DIR"
python3 scripts/publish-metropolis-release.py --artifacts "$ARTIFACTS_DIR" \
  --authorization "$AUTHORIZATION_JSON" --run-id "$RUN_ID"
```

CLI flags were checked using local `gh --help`; no workflow was dispatched.
`QUALIFICATION_REF` is a branch/tag, not an assumed dispatchable commit SHA.
Require run headSha to equal the approved commit. Check both native legs and comparison.
The last command is preflight only: it intentionally omits `--publish`.
Do not proceed until the extended report gates are implemented and tested.

## Executed Review Checks

`uv run --frozen pytest scripts/tests/test_metropolis_registry.py scripts/tests/test_metropolis_publish_release.py scripts/tests/test_release_readiness.py -q`

Result: **57 passed, 1 failed**. The failure is the missing-relayer inventory tripwire.
`git diff --check` passed. No native build or live RPC was run for this review.

## Product Gates After Release Qualification

Cold-history recovery, both installed live asset lifecycles, WMON unwrap and sponsored settlement remain open.
Independent setup, audit/retesting and outside acceptance remain separate requirements.
Submission video and portal evidence remain owner tasks. This review closes none of those gates.
