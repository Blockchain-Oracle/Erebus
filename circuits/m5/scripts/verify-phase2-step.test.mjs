import assert from "node:assert/strict";
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync, spawnSync } from "node:child_process";
import { validatePlan, verifyHistory } from "./verify-phase2-step.mjs";
import { hashFile } from "./build-artifact-bundle.mjs";

const entry = { name: "contributor-a", type: 0, hash: "11".repeat(64) };
const next = { name: "contributor-b", type: 0, hash: "22".repeat(64) };
const before = { circuit_hash: "same-circuit", contributions: [entry] };
const after = { circuit_hash: "same-circuit", contributions: [entry, next] };
assert.deepEqual(verifyHistory(before, after, "contributor-b"), next);
assert.throws(() => verifyHistory(before, before, "contributor-b"), /exactly one/);
assert.throws(() => verifyHistory(before, { ...after, circuit_hash: "other" }, "contributor-b"), /different circuit/);
assert.throws(() => verifyHistory(before, { ...after, contributions: [{ ...entry, hash: "changed" }, next] }, "contributor-b"), /history changed/);
assert.throws(() => verifyHistory(before, after, "stranger"), /different name/);
assert.throws(() => verifyHistory(before, { ...after, contributions: [entry, { ...next, type: 1 }] }, "contributor-b"), /beacon/);
const build = fileURLToPath(new URL("../build/", import.meta.url));
const temp = await mkdtemp(resolve(tmpdir(), "erebus-phase2-review-"));
try {
  const cli = fileURLToPath(new URL("./verify-phase2-step.mjs", import.meta.url));
  function refuses(path, message) {
    const result = spawnSync(process.execPath, [cli, path], { encoding: "utf8", timeout: 10000 });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr.trim(), message);
    assert.ok(!result.stderr.includes("PRIVATE"));
  }
  const malformed = resolve(temp, "malformed.json");
  await writeFile(malformed, '{"secret":"PRIVATE_STEP_ENTROPY",');
  refuses(malformed, "invalid step plan JSON");
  const linked = resolve(temp, "linked.json");
  await symlink(malformed, linked);
  refuses(linked, "invalid step plan file");
  const oversized = resolve(temp, "oversized.json");
  await writeFile(oversized, "PRIVATE".padEnd(65537, " "));
  refuses(oversized, "invalid step plan file");
  const empty = resolve(temp, "empty.json");
  await writeFile(empty, "");
  refuses(empty, "invalid step plan file");
  refuses(temp, "invalid step plan file");
  refuses(resolve(temp, "missing.json"), "invalid step plan file");
  const pinned = (digit) => ({ path: "unused", sha256: digit.repeat(64) });
  assert.equal(validatePlan({ version: 1, test_only: false, contributor: "future-release",
    r1cs: pinned("1"), ptau: pinned("2"), before: pinned("3"), after: pinned("4") }).test_only, false);
  if (process.argv.includes("--cli-only")) {
    console.log("phase-2 step focused CLI checks passed; no crypto run");
  } else {
  const plan = { version: 1, test_only: true, contributor: "m5-local-only" };
  for (const [key, name] of [["r1cs", "deposit.r1cs"], ["ptau", "pot16_final.ptau"], ["before", "deposit-initial.zkey"], ["after", "deposit.zkey"]]) {
    const path = resolve(build, name);
    plan[key] = { path, sha256: await hashFile(path) };
  }
  assert.throws(() => validatePlan({ ...plan, entropy: "NEVER-IN-A-PLAN" }), /fields/);
  assert.throws(() => validatePlan({ ...plan, after: plan.before }), /changed key/);
  const request = resolve(temp, "plan.json");
  await writeFile(request, JSON.stringify(plan));
  const output = execFileSync(process.execPath, [fileURLToPath(new URL("./verify-phase2-step.mjs", import.meta.url)), request], { encoding: "utf8", timeout: 180000 });
  const receipt = JSON.parse(output.trim().split("\n").at(-1));
  assert.equal(receipt.consistent, true);
  assert.equal(receipt.trust_verified, false);
  assert.equal(receipt.test_only_declared, true);
  assert.equal(receipt.contribution_count, 1);
  await writeFile(request, JSON.stringify({ ...plan, after: { ...plan.after, sha256: "00".repeat(32) } }));
  assert.throws(() => execFileSync(process.execPath, [fileURLToPath(new URL("./verify-phase2-step.mjs", import.meta.url)), request], { stdio: "pipe", timeout: 10000 }), (error) => /file identity mismatch/.test(error.stderr.toString()));
  // Existing fixture files must be unchanged by this verifier.
  assert.equal(await hashFile(plan.after.path), plan.after.sha256);
  console.log("phase-2 handoff checks passed on existing known-entropy fixture; no contribution generated");
  }
} finally { await rm(temp, { recursive: true, force: true }); }
process.exit(0);
