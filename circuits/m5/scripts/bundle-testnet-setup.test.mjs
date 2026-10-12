import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, symlink } from "node:fs/promises";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { bundleTestnetSetup } from "./bundle-testnet-setup.mjs";
import { hashFile } from "./build-artifact-bundle.mjs";

const temp = await mkdtemp("/private/tmp/erebus-bundle-tests-");
try {
  const path = resolve(temp, "receipt.json");
  const base = { test_only: true, receipt: { path, sha256: "11".repeat(32) }, base_url: "https://example.invalid/",
    chain_id: 10143, settlement_contract: `0x${"12".repeat(20)}`, verifier_version: 2 };
  for (const plan of [null, { ...base, test_only: false }, { ...base, extra: true }]) {
    await assert.rejects(bundleTestnetSetup(plan), /test-only bundle plan/);
  }
  await writeFile(path, "PRIVATE_INVALID_JSON");
  base.receipt.sha256 = await hashFile(path);
  await assert.rejects(bundleTestnetSetup(base), (error) => error.message === "invalid record JSON");
  const link = resolve(temp, "symlink.json");
  await symlink(path, link);
  await assert.rejects(bundleTestnetSetup({ ...base, receipt: { ...base.receipt, path: link } }), /invalid record file/);
  const receipt = { test_only: true, consistent: true, operator_count: 1, independent: false, trust_verified: false,
    destruction_verified: false, circuits: [{ kind: "deposit" }, { kind: "transfer" }, { kind: "withdraw" }] };
  for (const changed of [{ ...receipt, test_only: false }, { ...receipt, independent: true },
    { ...receipt, trust_verified: true }, { ...receipt, destruction_verified: true },
    { ...receipt, operator_count: 2 }, { ...receipt, consistent: false },
    { ...receipt, circuits: [receipt.circuits[0], receipt.circuits[0], receipt.circuits[2]] }]) {
    await writeFile(path, JSON.stringify(changed));
    await assert.rejects(bundleTestnetSetup({ ...base, receipt: { path, sha256: await hashFile(path) } }), /receipt|three circuit/);
  }
  await writeFile(path, JSON.stringify(receipt));
  await assert.rejects(bundleTestnetSetup({ ...base, receipt: { path, sha256: "00".repeat(32) } }), /pin mismatch/);
  await writeFile(path, "PRIVATE_INVALID_JSON");
  const script = fileURLToPath(new URL("./bundle-testnet-setup.mjs", import.meta.url));
  assert.throws(() => execFileSync(process.execPath, [script, path], { stdio: "pipe", timeout: 10000 }),
    (error) => error.status === 1 && error.stdout.length === 0 && error.stderr.toString() === "test-only bundle failed; no completion receipt issued\n");
  console.log("test-only setup bundle rejection gates passed");
} finally { await rm(temp, { recursive: true, force: true }); }
process.exit(0);
