import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { validatePlan, publishedDigest } from "./verify-phase1.mjs";
import { hashFile } from "./build-artifact-bundle.mjs";

const temp = await mkdtemp(resolve(tmpdir(), "erebus-phase1-review-"));
try {
  const ptau = fileURLToPath(new URL("../build/pot16_final.ptau", import.meta.url));
  const provenance = resolve(temp, "fixture-provenance.txt");
  await writeFile(provenance, "Known-entropy local fixture. Not selected release phase-1. No independence or destruction attested.\n");
  const plan = { version: 1, test_only: true, ptau: { path: ptau, sha256: await hashFile(ptau) }, provenance: { path: provenance, sha256: await hashFile(provenance) } };
  plan.published_pin = { algorithm: "blake2b512", digest: await publishedDigest(ptau, "blake2b512") };
  assert.throws(() => validatePlan({ ...plan, trust_verified: true }), /plan/);
  assert.throws(() => validatePlan({ ...plan, provenance: null }), /file identity/);
  assert.throws(() => validatePlan({ ...plan, published_pin: { algorithm: "sha512", digest: "11".repeat(64) } }), /published/);
  assert.throws(() => validatePlan({ ...plan, published_pin: { algorithm: "blake2b512", digest: "11".repeat(32) } }), /published/);
  const request = resolve(temp, "plan.json");
  await writeFile(request, JSON.stringify(plan));
  const command = fileURLToPath(new URL("./verify-phase1.mjs", import.meta.url));
  const output = execFileSync(process.execPath, [command, request], { encoding: "utf8", timeout: 240000 });
  const receipt = JSON.parse(output.trim().split("\n").at(-1));
  assert.equal(receipt.consistent, true);
  assert.equal(receipt.trust_verified, false);
  assert.equal(receipt.provenance_reviewed, false);
  assert.equal(receipt.test_only_declared, true);
  assert.deepEqual(receipt.published_pin, plan.published_pin);
  await writeFile(request, JSON.stringify({ ...plan, published_pin: { ...plan.published_pin, digest: "00".repeat(64) } }));
  assert.throws(() => execFileSync(process.execPath, [command, request], { stdio: "pipe", timeout: 10000 }), (error) => /published transcript digest mismatch/.test(error.stderr.toString()));
  await writeFile(request, JSON.stringify(plan));
  await writeFile(provenance, "changed record\n");
  assert.throws(() => execFileSync(process.execPath, [command, request], { stdio: "pipe", timeout: 10000 }), (error) => /identity mismatch/.test(error.stderr.toString()));
  assert.equal(await hashFile(ptau), plan.ptau.sha256);
  console.log("phase-1 consistency and provenance pin tests passed; existing known-entropy fixture unchanged");
} finally { await rm(temp, { recursive: true, force: true }); }
process.exit(0);
