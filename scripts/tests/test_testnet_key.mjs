import assert from "node:assert/strict";
import { mkdtemp, lstat, readFile, rm, symlink } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { prepareTestnetKey } from "../prepare-metropolis-testnet-key.mjs";

const directory = await mkdtemp("/private/tmp/erebus-testnet-key-test-");
const binary = resolve("sdk/evm/target/debug/erebus-settle");
const path = resolve(directory, "gas.key");
const digest = async () => createHash("sha256").update(await readFile(path)).digest("hex");
try {
  const result = await prepareTestnetKey(path, binary);
  assert.equal(result.chain_id, 10143);
  assert.equal(result.transactions_sent, 0);
  assert.equal(result.testnet_only, true);
  assert.match(result.address, /^0x[0-9a-f]{40}$/);
  assert.equal((await lstat(path)).mode & 0o777, 0o600);
  assert.equal((await lstat(path)).size, 32);
  const before = await digest();
  await assert.rejects(prepareTestnetKey(path, binary), { code: "EEXIST" });
  assert.equal(await digest(), before);
  const alias = resolve(directory, "alias.key");
  await symlink(path, alias);
  await assert.rejects(prepareTestnetKey(alias, binary), { code: "EEXIST" });
  assert.equal(await digest(), before);
  console.log("testnet key permission, derivation and no-overwrite checks passed");
} finally { await rm(directory, { recursive: true, force: true }); }
