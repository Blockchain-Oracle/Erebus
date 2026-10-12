import assert from "node:assert/strict";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { stageTestBundle, compilePinnedVerifier } from "./stage-test-bundle.mjs";
import { hashFile } from "./build-artifact-bundle.mjs";

const temp = await mkdtemp(resolve(tmpdir(), "erebus-stage-tests-"));
try {
  await assert.rejects(stageTestBundle("", "11".repeat(32)), /requires explicit/);
  const manifestPath = resolve(temp, "manifest.json");
  for (const bad of [{version: 2, test_only: false, circuits: []}, {version: 1, test_only: true, circuits: []},
    {version: 2, test_only: true, circuits: [{kind:"deposit"},{kind:"deposit"},{kind:"withdraw"}]}]) {
    await writeFile(manifestPath, JSON.stringify(bad));
    await assert.rejects(stageTestBundle(temp, await hashFile(manifestPath)), /three test-only/);
  }
  await writeFile(manifestPath, "PRIVATE_BAD_JSON");
  await assert.rejects(stageTestBundle(temp, await hashFile(manifestPath)), (error) => error.message === "invalid manifest JSON");
  await assert.rejects(stageTestBundle(temp, "00".repeat(32)), /pin mismatch/);
  assert.throws(() => compilePinnedVerifier("pragma solidity 0.8.24; contract Wrong {}", "00".repeat(32)), /runtime pin/);
  const supplied = process.env.EREBUS_M5_TEST_BUNDLE;
  if (supplied) {
    const manifestHash = process.env.EREBUS_M5_TEST_BUNDLE_SHA256;
    const result = await stageTestBundle(supplied, manifestHash);
    try {
      const manifest = JSON.parse(await readFile(resolve(supplied,"manifest.json"),"utf8"));
      assert.equal(result.artifacts.test_only, true);
      for (const circuit of manifest.circuits) {
        const name = circuit.kind;
        assert.equal(await hashFile(resolve(result.build, `${name}.zkey`)), circuit.zkey.sha256);
        assert.ok(result.verifiers[name].evm.bytecode.object.length > 0);
        const source = await readFile(resolve(result.build, `${name}-verifier.sol`), "utf8");
        assert.throws(() => compilePinnedVerifier(source + "\n// changed metadata\n", circuit.identity.verifier_runtime_keccak256), /runtime pin/);
      }
      assert.equal(await hashFile(resolve(supplied,"manifest.json")),manifestHash);
      const pool = fileURLToPath(new URL("./pool.mjs",import.meta.url));
      const printed = execFileSync(process.execPath,["--input-type=module","-e",`const {compileContracts}=await import(${JSON.stringify(pool)}); const c=compileContracts(); console.log(Object.keys(c).sort().join(',')); process.exit(0);`],{encoding:"utf8",timeout:60000});
      assert.ok(printed.includes("transfer-verifier.sol"));
      assert.ok(printed.includes("ErebusShieldedPool.sol"));
    } finally { await rm(result.build,{recursive:true,force:true}); }
    console.log("external test bundle staging and pool compiler passed");
  }
  console.log("test bundle rejection gates passed");
} finally {await rm(temp,{recursive:true,force:true});}
process.exit(0);
