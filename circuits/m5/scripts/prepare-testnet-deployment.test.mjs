import assert from "node:assert/strict";
import { readFile, writeFile, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { setTimeout as delay } from "node:timers/promises";
import { Contract, ContractFactory, JsonRpcProvider, keccak256, HDNodeWallet } from "ethers";
import solc from "solc";
import { prepareTestnetDeployment, validateDeploymentPlan, poseidonRuntime } from "./prepare-testnet-deployment.mjs";
import { checkTestnetDeployment, decodePinnedPlan } from "./check-testnet-deployment.mjs";
import { submitTestnetDeployment, validateSignedCreation } from "./submit-testnet-deployment.mjs";

const reviewedBytes = Buffer.from('{"version":1,"mode":"prepare_only"}\n');
const reviewedPin = createHash("sha256").update(reviewedBytes).digest("hex");
assert.equal(decodePinnedPlan(reviewedBytes, reviewedPin).mode, "prepare_only");
assert.throws(() => decodePinnedPlan(Buffer.from('{"version":1,"mode":"changed"}\n'), reviewedPin), /pin mismatch/);
assert.throws(() => decodePinnedPlan(Buffer.concat([reviewedBytes, Buffer.from(" ")]), reviewedPin), /pin mismatch/);
for (const invalidPin of [undefined, "bad", "00".repeat(32)]) {
  assert.throws(() => decodePinnedPlan(reviewedBytes, invalidPin), /pin mismatch/);
}

const root = fileURLToPath(new URL("../../../", import.meta.url));
const bundle = process.env.EREBUS_M5_TEST_BUNDLE;
const pin = process.env.EREBUS_M5_TEST_BUNDLE_SHA256;
const base = { version: 1, test_only: true, chain_id: 10143, bundle_dir: bundle || "unused",
  manifest_sha256: pin || "11".repeat(32), deployer: `0x${"12".repeat(20)}`, starting_nonce: 0,
  assets: [{ name: "erc20", address: `0x${"34".repeat(20)}`, runtime_keccak256: "22".repeat(32) }] };
validateDeploymentPlan(base);
for (const patch of [{ chain_id: 143 }, { test_only: false }, { starting_nonce: -1 },
  { starting_nonce: Number.MAX_SAFE_INTEGER }, { manifest_sha256: "bad" },
  { deployer: `0x${"00".repeat(20)}` }, { unknown: true }]) {
  assert.throws(() => validateDeploymentPlan({ ...base, ...patch }));
}
assert.throws(() => validateDeploymentPlan({ ...base, assets: [...base.assets, ...base.assets] }), /duplicate/);
assert.throws(() => poseidonRuntime("0x60006000f3"), /loader/);
assert.throws(() => poseidonRuntime("0x38600c6000396100016000f30000"), /loader/);

if (bundle && pin) {
  await assert.rejects(prepareTestnetDeployment({ ...base, manifest_sha256: "00".repeat(32) }), /pin/);
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise((done) => server.close(done));
  const anvil = spawn("anvil", ["--host", "127.0.0.1", "--port", String(port), "--chain-id", "10143",
    "--slots-in-an-epoch", "1", "--silent"],
    { stdio: "ignore" });
  const stopped = once(anvil, "exit");
  const provider = new JsonRpcProvider(`http://127.0.0.1:${port}`, undefined, { cacheTimeout: -1 });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      try { await provider.getBlockNumber(); ready = true; break; } catch { await delay(50); }
    }
    assert(ready, "local Anvil startup failed");
    const signer = await provider.getSigner(0);
    const deployer = await signer.getAddress();
    const sources = {};
    for (const name of ["MockERC20", "MockWMON"]) {
      sources[`${name}.sol`] = { content: await readFile(resolve(root, `contracts/evm/src/${name}.sol`), "utf8") };
    }
    const compiled = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity", sources,
      settings: { evmVersion: "paris", outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } } })));
    assert(!compiled.errors?.some((e) => e.severity === "error"));
    const assets = [];
    for (const [name, source, args] of [["erc20", "MockERC20", ["Test", "TEST"]], ["wmon", "MockWMON", []]]) {
      const artifact = compiled.contracts[`${source}.sol`][source];
      const token = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object, signer).deploy(...args);
      await token.waitForDeployment();
      const address = await token.getAddress();
      assets.push({ name, address, runtime_keccak256: keccak256(await provider.getCode(address)).slice(2) });
    }
    for (let count = 0; count < 4; count++) await provider.send("evm_mine", []);
    const nonce = await provider.getTransactionCount(deployer);
    const plan = await prepareTestnetDeployment({ ...base, deployer, starting_nonce: nonce, assets });
    assert.equal(plan.calls.length, 6);
    assert.equal(plan.live_transactions_sent, 0);
    assert.equal(plan.authorized_for_broadcast, false);
    assert.equal(await provider.getTransactionCount(deployer), nonce, "preparation sent a transaction");
    const journalRoot = await mkdtemp(resolve(tmpdir(), "erebus-deployment-journal-"));
    let hashes;
    try {
      const planBytes = Buffer.from(JSON.stringify(plan));
      const planPin = createHash("sha256").update(planBytes).digest("hex");
      const planFile = resolve(journalRoot, "prepared.json");
      const keyFile = resolve(journalRoot, "fixture.key");
      const fixtureKey = HDNodeWallet.fromPhrase("test test test test test test test test test test test junk");
      assert.equal(fixtureKey.address, deployer);
      await writeFile(planFile, planBytes, { flag: "wx", mode: 0o600 });
      await writeFile(keyFile, Buffer.from(fixtureKey.privateKey.slice(2), "hex"), { flag: "wx", mode: 0o600 });
      const urls = [`http://127.0.0.1:${port}`, `http://localhost:${port}`];
      const journal = resolve(journalRoot, "journal");
      await assert.rejects(submitTestnetDeployment(planFile, planPin, keyFile, journal, urls, false), /authorization/);
      // Mine local blocks only, so finalized receipt observation can progress without another transaction.
      let mining = false;
      const timer = setInterval(async () => {
        if (mining) return;
        mining = true;
        try { await provider.send("evm_mine", []); } finally { mining = false; }
      }, 500);
      try {
        hashes = await submitTestnetDeployment(planFile, planPin, keyFile, journal, urls, true);
        const completeNonce = await provider.getTransactionCount(deployer);
        assert.equal(completeNonce, nonce + 6);
        assert.deepEqual(await submitTestnetDeployment(planFile, planPin, keyFile, journal, urls, true), hashes);
        assert.equal(await provider.getTransactionCount(deployer), completeNonce, "resume redeployed a contract");
        const retained = JSON.parse(await readFile(resolve(journal, "0-signed.json"), "utf8"));
        assert.equal(validateSignedCreation(retained.raw, plan.calls[0]), hashes[0]);
        assert.throws(() => validateSignedCreation(retained.raw, { ...plan.calls[0], nonce: 99 }), /reviewed intent/);
        assert.throws(() => validateSignedCreation(retained.raw, { ...plan.calls[0], data: "0x00" }), /reviewed intent/);
      } finally { clearInterval(timer); }
    } finally { await rm(journalRoot, { recursive: true, force: true }); }
    const abi = ["function asset() view returns(address)", "function poseidonTwo() view returns(address)",
      "function depositVerifier() view returns(address)", "function transferVerifier() view returns(address)",
      "function withdrawVerifier() view returns(address)", "function verifierVersion() view returns(uint256)",
      "function nextLeafIndex() view returns(uint256)", "function currentRoot() view returns(uint256)"];
    for (const pool of plan.pools) {
      const contract = new Contract(pool.pool, abi, provider);
      assert.equal((await contract.asset()).toLowerCase(), pool.asset.toLowerCase());
      assert.equal((await contract.poseidonTwo()).toLowerCase(), plan.calls[0].predicted_address.toLowerCase());
      for (const [method, name] of [["depositVerifier", "deposit"], ["transferVerifier", "transfer"], ["withdrawVerifier", "withdraw"]]) {
        assert.equal((await contract[method]()).toLowerCase(), plan.calls.find((c) => c.name === name).predicted_address.toLowerCase());
      }
      assert.equal(await contract.verifierVersion(), 2n);
      assert.equal(await contract.nextLeafIndex(), 0n);
      assert((await contract.currentRoot()) > 0n);
    }
    // Leave room for both Anvil finality and the second provider's honest lag.
    for (let count = 0; count < 4; count++) await provider.send("evm_mine", []);
    const readOnly = new Set(["eth_chainId", "eth_getBlockByNumber", "eth_getTransactionReceipt",
      "eth_getTransactionByHash", "eth_getCode", "eth_call"]);
    const observed = [];
    const reader = (mutate = (_, __, result) => result) => ({ async send(method, params) {
      assert(readOnly.has(method), "verification attempted a write");
      if (method === "eth_getCode" || method === "eth_call") {
        assert.equal(params[1].requireCanonical, true);
        assert.match(params[1].blockHash, /^0x[0-9a-f]{64}$/i);
      }
      observed.push(method);
      return mutate(method, params, await provider.send(method, params));
    } });
    const beforeCheck = await provider.getTransactionCount(deployer);
    const verified = await checkTestnetDeployment(plan, hashes, [reader(), reader()]);
    assert.equal(verified.creations.length, 6);
    assert(verified.pools.every((p) => p.deployment_block > 0 && p.deployment_hash));
    assert.equal(await provider.getTransactionCount(deployer), beforeCheck);
    assert(observed.includes("eth_call"));
    const lagged = reader(async (method, params, result) => method === "eth_getBlockByNumber" && params[0] === "finalized"
      ? provider.send(method, [`0x${(BigInt(result.number) - 1n).toString(16)}`, false]) : result);
    const laggedResult = await checkTestnetDeployment(plan, hashes, [reader(), lagged]);
    assert(laggedResult.anchor.block < verified.anchor.block);
    for (const mutation of [
      (method, params, result) => method === "eth_getBlockByNumber" && params[0] === "finalized" ? null : result,
      (method, params, result) => method === "eth_getBlockByNumber" && params[0] === "finalized"
        ? { ...result, hash: `0x${"34".repeat(32)}` } : result,
      (method, _, result) => method === "eth_chainId" ? "0x1" : result,
      (method, _, result) => method === "eth_getCode" ? "0x00" : result,
      (method, _, result) => method === "eth_getTransactionReceipt" ? { ...result, status: "0x0" } : result,
      (method, _, result) => method === "eth_getTransactionByHash" ? { ...result, input: "0x00" } : result,
      (method, params, result) => method === "eth_getBlockByNumber" && params[0] !== "finalized"
        ? { ...result, hash: `0x${"12".repeat(32)}` } : result,
    ]) {
      await assert.rejects(checkTestnetDeployment(plan, hashes, [reader(), reader(mutation)]));
    }
    const badPins = structuredClone(plan);
    badPins.pools[0].runtime_pins.pool = "00".repeat(32);
    await assert.rejects(checkTestnetDeployment(badPins, hashes, [reader(), reader()]), /configuration identity/);
    console.log("six local deployments match predicted addresses, immutable-bound runtimes and getters; no live chain used");
    console.log("paired finalized read-only verification and provider-mutation rejection tests passed");
  } finally {
    provider.destroy();
    if (anvil.exitCode === null) anvil.kill("SIGTERM");
    await stopped;
  }
} else {
  console.log("bundle-backed Anvil deployment test not run; set both explicit bundle variables");
}
console.log("deployment plan rejection tests passed");
process.exit(0);
