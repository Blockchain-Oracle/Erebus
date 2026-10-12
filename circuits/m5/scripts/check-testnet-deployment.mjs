// Read-only verification. Both providers must expose explicit finalized, canonical evidence.
import { readFile, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Interface, JsonRpcProvider, keccak256, getCreateAddress } from "ethers";

const hash = (value) => typeof value === "string" && /^0x[0-9a-f]{64}$/i.test(value);
const lower = (value) => typeof value === "string" ? value.toLowerCase() : value;
const quantity = (value) => {
  if (typeof value !== "string" || !/^0x(?:0|[1-9a-f][0-9a-f]*)$/i.test(value)) throw new Error("invalid RPC quantity");
  return BigInt(value);
};
const blockTag = (value) => `0x${value.toString(16)}`;
const getters = new Interface(["function asset() view returns(address)", "function poseidonTwo() view returns(address)",
  "function depositVerifier() view returns(address)", "function transferVerifier() view returns(address)",
  "function withdrawVerifier() view returns(address)", "function verifierVersion() view returns(uint256)"]);

export function decodePinnedPlan(bytes, expectedSha256) {
  if (typeof expectedSha256 !== "string" || !/^[0-9a-f]{64}$/.test(expectedSha256)
      || createHash("sha256").update(bytes).digest("hex") !== expectedSha256) {
    throw new Error("reviewed deployment plan pin mismatch");
  }
  return JSON.parse(bytes.toString("utf8"));
}

export async function checkTestnetDeployment(plan, transactionHashes, providers) {
  if (!plan || plan.version !== 1 || plan.mode !== "prepare_only" || plan.test_only !== true
      || plan.authorized_for_broadcast !== false || !Array.isArray(plan.calls) || plan.calls.length < 5
      || plan.calls.length > 6 || !Array.isArray(plan.pools) || plan.pools.length !== plan.calls.length - 4
      || !Array.isArray(providers) || providers.length !== 2 || providers[0] === providers[1]
      || !Array.isArray(transactionHashes) || transactionHashes.length !== plan.calls.length
      || !transactionHashes.every(hash) || new Set(transactionHashes.map(lower)).size !== transactionHashes.length) {
    throw new Error("invalid prepared deployment evidence request");
  }
  async function pair(method, params) {
    const results = [];
    for (const provider of providers) results.push(await provider.send(method, params));
    return results;
  }
  async function finalizedTips() {
    const tips = await pair("eth_getBlockByNumber", ["finalized", false]);
    for (const [index, tip] of tips.entries()) {
      if (!tip || !hash(tip.hash)) throw new Error("explicit finalized evidence unavailable");
      const number = quantity(tip.number);
      const canonical = await providers[index].send("eth_getBlockByNumber", [blockTag(number), false]);
      if (!canonical || quantity(canonical.number) !== number || lower(canonical.hash) !== lower(tip.hash)) {
        throw new Error("finalized tip is not canonical");
      }
    }
    return tips;
  }
  if (plan.calls.slice(0, 4).map((c) => c.name).join() !== "poseidon,deposit,transfer,withdraw") {
    throw new Error("invalid deployment order");
  }
  for (const [index, call] of plan.calls.entries()) {
    if (!Number.isSafeInteger(call.nonce) || call.nonce < 0
        || call.nonce !== plan.calls[0].nonce + index || lower(call.from) !== lower(plan.calls[0].from)
        || lower(getCreateAddress({ from: call.from, nonce: call.nonce })) !== lower(call.predicted_address)) {
      throw new Error("planned deployment identity mismatch");
    }
  }
  if (!(await pair("eth_chainId", [])).every((id) => quantity(id) === 10143n)) throw new Error("wrong chain");
  const finalized = await finalizedTips();
  const heights = finalized.map((b) => quantity(b.number));
  const height = heights[0] < heights[1] ? heights[0] : heights[1];
  const anchors = await pair("eth_getBlockByNumber", [blockTag(height), false]);
  if (anchors.some((b) => !b || !hash(b.hash) || quantity(b.number) !== height)
      || lower(anchors[0].hash) !== lower(anchors[1].hash)) throw new Error("finalized providers disagree");
  const anchor = { blockHash: anchors[0].hash, requireCanonical: true };
  const creations = [];
  for (let index = 0; index < plan.calls.length; index++) {
    const expected = plan.calls[index];
    if (expected.chain_id !== 10143 || expected.value !== "0" || !hash(expected.runtime_keccak256)
        || keccak256(expected.data) !== expected.initcode_keccak256) throw new Error("invalid planned call");
    const receipts = await pair("eth_getTransactionReceipt", [transactionHashes[index]]);
    for (const receipt of receipts) {
      if (!receipt) throw new Error("creation receipt unavailable");
      if (quantity(receipt.status) !== 1n) throw new Error("creation reverted");
      if (!hash(receipt.blockHash)) throw new Error("creation block hash invalid");
      if (quantity(receipt.blockNumber) > height) throw new Error("creation not finalized");
      if (receipt.to !== null) throw new Error("creation receipt has a recipient");
      if (lower(receipt.from) !== lower(expected.from)) throw new Error("creation receipt sender mismatch");
      if (lower(receipt.contractAddress) !== lower(expected.predicted_address)) throw new Error("creation address mismatch");
      if (lower(receipt.transactionHash) !== lower(transactionHashes[index])) throw new Error("creation receipt transaction mismatch");
    }
    if (lower(receipts[0].blockHash) !== lower(receipts[1].blockHash)
        || receipts[0].blockNumber !== receipts[1].blockNumber) throw new Error("creation providers disagree");
    const blocks = await pair("eth_getBlockByNumber", [receipts[0].blockNumber, false]);
    if (blocks.some((b) => !b || lower(b.hash) !== lower(receipts[0].blockHash))) throw new Error("creation block not canonical");
    for (const tx of await pair("eth_getTransactionByHash", [transactionHashes[index]])) {
      if (!tx || tx.to !== null || lower(tx.from) !== lower(expected.from)
          || quantity(tx.nonce) !== BigInt(expected.nonce) || quantity(tx.value) !== 0n
          || quantity(tx.chainId) !== 10143n || lower(tx.input) !== lower(expected.data)
          || lower(tx.blockHash) !== lower(receipts[0].blockHash)
          || lower(tx.hash) !== lower(transactionHashes[index])) throw new Error("creation transaction mismatch");
    }
    for (const code of await pair("eth_getCode", [expected.predicted_address, anchor])) {
      if (keccak256(code) !== expected.runtime_keccak256) throw new Error("deployed runtime mismatch");
    }
    creations.push({ name: expected.name, address: expected.predicted_address,
      tx_hash: transactionHashes[index], block: Number(quantity(receipts[0].blockNumber)),
      block_hash: receipts[0].blockHash, runtime_keccak256: expected.runtime_keccak256 });
  }
  for (const pool of plan.pools) {
    const expectedCall = plan.calls.find((c) => c.name === `pool_${pool.name}`);
    if (!expectedCall || lower(pool.pool) !== lower(expectedCall.predicted_address)
        || pool.verifier_version !== 2 || pool.namespace !== "eip155:10143"
        || pool.runtime_pins?.pool !== expectedCall.runtime_keccak256.slice(2)
        || plan.calls.slice(0, 4).some((c) => pool.runtime_pins[c.name] !== c.runtime_keccak256.slice(2))) {
      throw new Error("pool configuration identity mismatch");
    }
    for (const code of await pair("eth_getCode", [pool.asset, anchor])) {
      if (keccak256(code).slice(2) !== pool.asset_runtime_keccak256) throw new Error("asset runtime mismatch");
    }
    const values = { asset: pool.asset, poseidonTwo: plan.calls[0].predicted_address,
      depositVerifier: plan.calls[1].predicted_address, transferVerifier: plan.calls[2].predicted_address,
      withdrawVerifier: plan.calls[3].predicted_address, verifierVersion: 2n };
    for (const [method, expected] of Object.entries(values)) {
      const data = getters.encodeFunctionData(method);
      for (const returned of await pair("eth_call", [{ to: pool.pool, data }, anchor])) {
        const value = getters.decodeFunctionResult(method, returned)[0];
        if (lower(value) !== lower(expected)) throw new Error("pool constructor getter mismatch");
      }
    }
  }
  const rechecked = await pair("eth_getBlockByNumber", [blockTag(height), false]);
  if (rechecked.some((b) => !b || lower(b.hash) !== lower(anchor.blockHash))) throw new Error("anchor changed during verification");
  const finalTips = await finalizedTips();
  if (finalTips.some((b) => !b || !hash(b.hash) || quantity(b.number) < height)
      || !(await pair("eth_chainId", [])).every((id) => quantity(id) === 10143n)) throw new Error("finality or chain changed");
  return { version: 1, chain_id: 10143, test_only: true, audited: false, live_transactions_sent: 0,
    observation: "paired-provider-finalized", anchor: { block: Number(height), hash: anchor.blockHash },
    manifest_sha256: plan.manifest_sha256, creations,
    pools: plan.pools.map((pool) => {
      const creation = creations.find((c) => c.address === pool.pool);
      return { ...pool, deployment_block: creation.block, deployment_hash: creation.block_hash };
    }), artifact_domain_requires_rebinding: true };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const providers = [];
  try {
    if (process.argv.length !== 8) throw new Error("invalid arguments");
    const planSha256 = process.argv[7];
    const plan = decodePinnedPlan(await readFile(process.argv[2]), planSha256);
    const urls = process.argv.slice(4, 6).map((value) => new URL(value));
    if (urls[0].href === urls[1].href || urls.some((u) => !["http:", "https:"].includes(u.protocol))) {
      throw new Error("two distinct RPC endpoints required");
    }
    for (const url of urls) providers.push(new JsonRpcProvider(url.href, undefined, { cacheTimeout: -1 }));
    const hashes = JSON.parse(await readFile(process.argv[3], "utf8"));
    const result = await checkTestnetDeployment(plan, hashes, providers);
    result.prepared_plan_sha256 = planSha256;
    await writeFile(process.argv[6], `${JSON.stringify(result, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    console.log(JSON.stringify({ status: "verified", contracts: result.creations.length, anchor: result.anchor, live_transactions_sent: 0 }));
  } catch { console.error("testnet deployment verification failed; no configuration issued"); process.exitCode = 1; }
  finally { providers.forEach((provider) => provider.destroy()); }
}
