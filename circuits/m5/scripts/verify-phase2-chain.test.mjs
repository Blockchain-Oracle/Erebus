import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import * as snarkjs from "snarkjs";
import { hashFile } from "./build-artifact-bundle.mjs";
import { validatePlan, validateSequence, verifyChain } from "./verify-phase2-chain.mjs";

const kinds = ["deposit", "transfer", "withdraw"];
const hash = "11".repeat(32);
const contributors = ["fixture-chain-a", "fixture-chain-b"];
const sample = { version: 1, test_only: true, contributors, ptau_sha256: hash,
  circuits: kinds.map((kind) => ({ kind, r1cs_sha256: hash, steps: contributors.map((name) => ({ path: name, sha256: hash })) })) };
assert.equal(validatePlan(sample), sample);
assert.equal(validatePlan({ ...sample, test_only: false }).test_only, false);
for (const malformed of [null, { ...sample, trust_verified: true },
  { ...sample, contributors: ["same", "Same"] }, { ...sample, contributors: [" one", "two"] },
  { ...sample, contributors: ["one"] }, { ...sample, circuits: sample.circuits.slice(1) },
  { ...sample, circuits: [sample.circuits[0], sample.circuits[0], sample.circuits[2]] },
  { ...sample, ptau_sha256: hash.toUpperCase().replace("1", "A") },
  { ...sample, circuits: [{ ...sample.circuits[0], steps: [] }, ...sample.circuits.slice(1)] }]) {
  assert.throws(() => validatePlan(malformed));
}
const record = (sha256) => ({ path: "fixture", sha256 });
const steps = contributors.map((contributor, index) => ({ version: 1, test_only: true, contributor,
  r1cs: record(hash), ptau: record(hash), before: record(String(index + 2).repeat(64)), after: record(String(index + 3).repeat(64)) }));
validateSequence(sample, sample.circuits[0], steps);
validateSequence({ ...sample, test_only: false }, sample.circuits[0], steps.map((step) => ({ ...step, test_only: false })));
for (const [field, value] of [["contributor", "stranger"], ["test_only", false], ["r1cs", record("22".repeat(32))],
  ["ptau", record("22".repeat(32))], ["before", record("66".repeat(32))]]) {
  const changed = structuredClone(steps);
  changed[1][field] = value;
  assert.throws(() => validateSequence(sample, sample.circuits[0], changed));
}
assert.throws(() => validateSequence(sample, sample.circuits[0], steps.slice(1)));

const build = fileURLToPath(new URL("../build/", import.meta.url));
const temp = await mkdtemp(resolve(tmpdir(), "erebus-phase2-chain-fixture-"));
try {
  const cli = fileURLToPath(new URL("./verify-phase2-chain.mjs", import.meta.url));
  function refuses(path, message) {
    const result = spawnSync(process.execPath, [cli, path], { encoding: "utf8", timeout: 10000 });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.equal(result.stderr.trim(), message);
    assert.ok(!result.stderr.includes("PRIVATE"));
  }
  const malformedPath = resolve(temp, "malformed.json");
  await writeFile(malformedPath, "PRIVATE_NOT_JSON");
  const malformedRecord = { path: malformedPath, sha256: await hashFile(malformedPath) };
  const malformedPlan = structuredClone(sample);
  for (const circuit of malformedPlan.circuits) circuit.steps = contributors.map(() => malformedRecord);
  await assert.rejects(verifyChain(malformedPlan), (error) => error.message === "invalid step record JSON");
  refuses(malformedPath, "invalid chain plan JSON");
  const request = resolve(temp, "plan.json");
  await writeFile(request, JSON.stringify(malformedPlan));
  refuses(request, "invalid step record JSON");
  const wrongHash = structuredClone(malformedPlan);
  for (const circuit of wrongHash.circuits) for (const step of circuit.steps) step.sha256 = "00".repeat(32);
  await writeFile(request, JSON.stringify(wrongHash));
  refuses(request, "step record identity mismatch");
  const linked = resolve(temp, "linked.json");
  await symlink(malformedPath, linked);
  const oversized = resolve(temp, "oversized.json");
  await writeFile(oversized, "PRIVATE".padEnd(65537, " "));
  const empty = resolve(temp, "empty.json");
  await writeFile(empty, "");
  for (const path of [linked, oversized, empty, temp, resolve(temp, "missing.json")]) {
    refuses(path, "invalid chain plan file");
    const invalidRecord = structuredClone(malformedPlan);
    for (const circuit of invalidRecord.circuits) for (const step of circuit.steps) step.path = path;
    await writeFile(request, JSON.stringify(invalidRecord));
    refuses(request, "step record identity mismatch");
  }
  // Valid, matching record bytes reach sequence validation without invoking crypto.
  await writeFile(malformedPath, JSON.stringify({ ...steps[0], contributor: "unexpected" }));
  const validBytesPlan = structuredClone(malformedPlan);
  for (const circuit of validBytesPlan.circuits) for (const step of circuit.steps) step.sha256 = await hashFile(malformedPath);
  await writeFile(request, JSON.stringify(validBytesPlan));
  refuses(request, "step does not match ceremony pins or contributor order");
  if (process.argv.includes("--cli-only")) {
    console.log("phase-2 chain focused CLI and record checks passed; no crypto run");
  } else {
  const ptauPath = resolve(build, "pot16_final.ptau");
  const ptau = { path: ptauPath, sha256: await hashFile(ptauPath) };
  const plan = { version: 1, test_only: true, contributors, ptau_sha256: ptau.sha256, circuits: [] };
  const retained = new Map([[ptau.path, ptau.sha256]]);
  for (const kind of kinds) {
    const r1cs = { path: resolve(build, `${kind}.r1cs`), sha256: await hashFile(resolve(build, `${kind}.r1cs`)) };
    let before = { path: resolve(build, `${kind}-initial.zkey`), sha256: await hashFile(resolve(build, `${kind}-initial.zkey`)) };
    retained.set(r1cs.path, r1cs.sha256);
    retained.set(before.path, before.sha256);
    const circuit = { kind, r1cs_sha256: r1cs.sha256, steps: [] };
    for (let index = 0; index < contributors.length; index++) {
      const path = resolve(temp, `${kind}-${index + 1}.zkey`);
      // Local test keys only. Two labels on one machine do not imply independence.
      await snarkjs.zKey.contribute(before.path, path, contributors[index], `EREBUS_LOCAL_CHAIN_FIXTURE_ONLY_${kind}_${index}`);
      const after = { path, sha256: await hashFile(path) };
      const step = { version: 1, test_only: true, contributor: contributors[index], r1cs, ptau, before, after };
      const stepPath = resolve(temp, `${kind}-${index + 1}.json`);
      await writeFile(stepPath, JSON.stringify(step));
      circuit.steps.push({ path: stepPath, sha256: await hashFile(stepPath) });
      before = after;
    }
    plan.circuits.push(circuit);
  }
  const receipt = await verifyChain(plan);
  assert.equal(receipt.consistent, true);
  assert.equal(receipt.test_only_declared, true);
  assert.equal(receipt.trust_verified, false);
  assert.equal(receipt.circuits.length, 3);
  for (const circuit of receipt.circuits) {
    assert.deepEqual(circuit.steps.map((step) => step.contribution_count), [1, 2]);
    assert.deepEqual(circuit.steps.map((step) => step.contribution.name), contributors);
    assert.equal(circuit.final_zkey_sha256, circuit.steps[1].after_sha256);
  }
  const tampered = structuredClone(plan);
  tampered.circuits[0].steps[0].sha256 = "00".repeat(32);
  await assert.rejects(verifyChain(tampered), /record identity mismatch/);
  const path = plan.circuits[0].steps[0].path;
  const changed = JSON.parse(await readFile(path, "utf8"));
  changed.contributor = "other-person";
  await writeFile(path, JSON.stringify(changed));
  const relabelled = structuredClone(plan);
  relabelled.circuits[0].steps[0].sha256 = await hashFile(path);
  await assert.rejects(verifyChain(relabelled), /contributor order/);
  for (const [path, sha256] of retained) assert.equal(await hashFile(path), sha256);
  console.log("three-circuit two-step fixture chain verified; no independent contribution or release trust claimed");
  }
} finally { await rm(temp, { recursive: true, force: true }); }
process.exit(0);
