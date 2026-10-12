import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile, mkdir, copyFile, symlink, truncate } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { artifactIdentity, copyPinnedArtifact, sourceInventory, validatePlan, hashFile } from "./build-artifact-bundle.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const temp = await mkdtemp(resolve(tmpdir(), "erebus-m5-bundle-"));
try {
  const plan = { test_only: true, build_dir: resolve(root, "build"), output_dir: resolve(temp, "bundle"), base_url: "https://example.invalid/fixtures/", chain_id: 10143, settlement_contract: `0x${"12".repeat(20)}`, verifier_version: 2 };
  assert.throws(() => validatePlan({ ...plan, test_only: false }), /only test-only/);
  assert.throws(() => validatePlan({ ...plan, base_url: "https://user:secret@example.invalid/" }), /base_url/);
  assert.throws(() => validatePlan({ ...plan, chain_id: 1.5 }), /chain_id/);
  assert.throws(() => validatePlan({ ...plan, unknown: true }), /plan fields/);
  const publicFile = resolve(temp, "public-file");
  await writeFile(publicFile, "reviewed artifact");
  const pin = await artifactIdentity(publicFile);
  const copiedFile = resolve(temp, "copied/artifact");
  await copyPinnedArtifact(publicFile, copiedFile, pin);
  assert.deepEqual(await artifactIdentity(copiedFile), pin);
  await assert.rejects(copyPinnedArtifact(publicFile, copiedFile, pin), { code: "EEXIST" });
  await writeFile(publicFile, "changed artifact");
  await assert.rejects(copyPinnedArtifact(publicFile, resolve(temp, "changed-copy"), pin), /identity mismatch/);
  const alias = resolve(temp, "alias");
  await symlink(publicFile, alias);
  await assert.rejects(artifactIdentity(alias), /invalid artifact/);
  await truncate(publicFile, 0);
  await assert.rejects(artifactIdentity(publicFile), /invalid artifact/);
  await truncate(publicFile, 256 * 1024 * 1024 + 1);
  await assert.rejects(artifactIdentity(publicFile), /invalid artifact/);
  const inventory = await sourceInventory();
  assert(inventory.files.some((file) => file.path === "lib/eddsa_strict.circom"));
  assert(inventory.files.some((file) => file.path === "node_modules/circomlib/circuits/poseidon.circom"));
  assert.deepEqual(inventory, await sourceInventory());
  const fake = resolve(temp, "source");
  await mkdir(fake);
  for (const kind of ["deposit", "transfer", "withdraw"]) await writeFile(resolve(fake, `${kind}.circom`), 'include "../outside.circom";');
  await writeFile(resolve(temp, "outside.circom"), "pragma circom 2.2.3;");
  await assert.rejects(sourceInventory(fake), /escapes/);
  const planPath = resolve(temp, "plan.json");
  const stale = resolve(temp, "stale");
  for (const kind of ["deposit", "transfer", "withdraw"]) {
    await mkdir(resolve(stale, `${kind}_js`), { recursive: true });
    for (const name of [`${kind}_js/${kind}.wasm`, `${kind}.r1cs`, `${kind}.zkey`, `${kind}-verification-key.json`]) {
      await copyFile(resolve(plan.build_dir, name), resolve(stale, name));
    }
  }
  await copyFile(resolve(plan.build_dir, "pot16_final.ptau"), resolve(stale, "pot16_final.ptau"));
  await writeFile(resolve(stale, "deposit.r1cs"), "stale artifact");
  await writeFile(planPath, JSON.stringify({ ...plan, build_dir: stale }));
  assert.throws(() => execFileSync(process.execPath, [resolve(root, "scripts/build-artifact-bundle.mjs"), planPath], { stdio: "pipe", timeout: 60000 }), (error) => /source rebuild mismatch/.test(error.stderr.toString()));
  await writeFile(planPath, JSON.stringify(plan));
  execFileSync(process.execPath, [resolve(root, "scripts/build-artifact-bundle.mjs"), planPath], { stdio: "inherit", timeout: 180000 });
  const manifest = JSON.parse(await readFile(resolve(plan.output_dir, "manifest.json"), "utf8"));
  assert.equal(manifest.test_only, true);
  assert.equal(manifest.version, 2);
  assert.equal(manifest.circuits.length, 3);
  const sourceHash = await hashFile(resolve(plan.output_dir, "source-inventory.json"));
  const recorded = JSON.parse(await readFile(resolve(plan.output_dir, "source-inventory.json"), "utf8"));
  assert.equal(recorded.toolchain.source_rebuild_matches, true);
  assert.deepEqual(Object.keys(recorded.toolchain.executable_sha256).sort(), ["circom_binary", "node_binary", "solc_engine"]);
  for (const hash of Object.values(recorded.toolchain.executable_sha256)) assert.match(hash, /^[0-9a-f]{64}$/);
  assert.equal(recorded.witness_sources.length, 5);
  for (const circuit of manifest.circuits) {
    assert.equal(circuit.identity.source_inventory_sha256, sourceHash);
    assert.match(circuit.identity.verifier_runtime_keccak256, /^[0-9a-f]{64}$/);
    assert.equal(circuit.identity.witness_schema_version, 2);
    assert.equal(circuit.zkey.sha256, await hashFile(resolve(plan.build_dir, `${circuit.kind}.zkey`)));
    for (const [field, name] of [["wasm", `${circuit.kind}_js/${circuit.kind}.wasm`], ["r1cs", `${circuit.kind}.r1cs`], ["zkey", `${circuit.kind}.zkey`]]) {
      assert.deepEqual(await artifactIdentity(resolve(plan.output_dir, name)), { sha256: circuit[field].sha256, bytes: circuit[field].bytes });
      assert.equal(circuit[field].url, new URL(name, plan.base_url).href);
    }
    assert.equal(await hashFile(resolve(plan.output_dir, `${circuit.kind}-verification-key.json`)), circuit.identity.verification_key_sha256);
  }
  assert.equal(await hashFile(resolve(plan.output_dir, "pot16_final.ptau")), recorded.toolchain.ptau_sha256);
  const manifestHash = await hashFile(resolve(plan.output_dir, "manifest.json"));
  assert.throws(() => execFileSync(process.execPath, [resolve(root, "scripts/build-artifact-bundle.mjs"), planPath], { stdio: "pipe", timeout: 180000 }), (error) => /EEXIST/.test(error.stderr.toString()));
  assert.equal(await hashFile(resolve(plan.output_dir, "manifest.json")), manifestHash);
  console.log("fixture bundle gates passed; no release trust claimed");
} finally { await rm(temp, { recursive: true, force: true }); }
process.exit(0);
