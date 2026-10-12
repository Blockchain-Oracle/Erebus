// Explicitly authorized testnet execution. Retained bytes are never replaced after an attempt.
import { readFile, mkdir, open, lstat, rmdir } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Wallet, Transaction, JsonRpcProvider, keccak256, getCreateAddress } from "ethers";
import { decodePinnedPlan } from "./check-testnet-deployment.mjs";

export function validateSignedCreation(raw, expected) {
  const tx = Transaction.from(raw);
  if (!tx.isSigned() || tx.type !== 2 || tx.chainId !== 10143n || tx.to !== null
      || tx.value !== 0n || tx.nonce !== expected.nonce
      || tx.from.toLowerCase() !== expected.from.toLowerCase()
      || tx.data.toLowerCase() !== expected.data.toLowerCase()
      || keccak256(tx.data) !== expected.initcode_keccak256
      || getCreateAddress({ from: tx.from, nonce: tx.nonce }).toLowerCase() !== expected.predicted_address.toLowerCase()) {
    throw new Error("retained creation differs from reviewed intent");
  }
  return tx.hash;
}

async function privateDirectory(path) {
  await mkdir(path, { mode: 0o700, recursive: true });
  const stat = await lstat(path);
  if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o077)
      || (process.getuid && stat.uid !== process.getuid())) throw new Error("journal directory is not owner-only");
}

async function existing(path) {
  try {
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink() || (stat.mode & 0o077)
        || (process.getuid && stat.uid !== process.getuid())) throw new Error("unsafe retained record");
    return JSON.parse(await readFile(path, "utf8"));
  } catch (error) { if (error.code === "ENOENT") return null; throw error; }
}

async function durableCreate(directory, name, record) {
  const file = await open(resolve(directory, name), "wx", 0o600);
  try { await file.writeFile(`${JSON.stringify(record)}\n`); await file.sync(); }
  finally { await file.close(); }
  const parent = await open(directory, "r");
  try { await parent.sync(); } finally { await parent.close(); }
}

async function pair(providers, method, params) {
  const results = [];
  for (const p of providers) results.push(await p.send(method, params));
  return results;
}

async function chain(providers) {
  if (!(await pair(providers, "eth_chainId", [])).every((x) => BigInt(x) === 10143n)) throw new Error("wrong chain");
}

async function finalizedCreation(providers, txHash, expected) {
  const receipts = await pair(providers, "eth_getTransactionReceipt", [txHash]);
  if (receipts.some((r) => !r)) return null;
  const tips = await pair(providers, "eth_getBlockByNumber", ["finalized", false]);
  if (tips.some((b) => !b?.hash || !b.number)) throw new Error("explicit finality unavailable");
  for (const [i, tip] of tips.entries()) {
    const canonical = await providers[i].send("eth_getBlockByNumber", [tip.number, false]);
    if (canonical?.hash !== tip.hash) throw new Error("finalized tip is not canonical");
  }
  if (receipts.some((r, i) => BigInt(r.blockNumber) > BigInt(tips[i].number))) return null;
  if (receipts.some((r) => BigInt(r.status) !== 1n)) throw new Error("creation reverted; do not advance plan");
  if (receipts[0].blockHash !== receipts[1].blockHash || receipts[0].blockNumber !== receipts[1].blockNumber
      || receipts.some((r) => r.to !== null || r.transactionHash.toLowerCase() !== txHash.toLowerCase()
        || r.from.toLowerCase() !== expected.from.toLowerCase()
        || r.contractAddress?.toLowerCase() !== expected.predicted_address.toLowerCase())) throw new Error("creation receipts disagree");
  const tag = { blockHash: receipts[0].blockHash, requireCanonical: true };
  for (const p of providers) {
    const block = await p.send("eth_getBlockByNumber", [receipts[0].blockNumber, false]);
    if (block?.hash !== receipts[0].blockHash) throw new Error("creation not canonical");
    const tx = await p.send("eth_getTransactionByHash", [txHash]);
    if (!tx || tx.to !== null || tx.from.toLowerCase() !== expected.from.toLowerCase()
        || BigInt(tx.chainId) !== 10143n || BigInt(tx.nonce) !== BigInt(expected.nonce)
        || BigInt(tx.value) !== 0n || tx.input.toLowerCase() !== expected.data.toLowerCase()
        || tx.blockHash !== receipts[0].blockHash) throw new Error("creation transaction mismatch");
    if (keccak256(await p.send("eth_getCode", [expected.predicted_address, tag])) !== expected.runtime_keccak256) {
      throw new Error("creation runtime mismatch");
    }
    const recheck = await p.send("eth_getBlockByNumber", [receipts[0].blockNumber, false]);
    if (recheck?.hash !== receipts[0].blockHash) throw new Error("creation anchor changed");
    const final = await p.send("eth_getBlockByNumber", ["finalized", false]);
    if (!final?.number || BigInt(final.number) < BigInt(receipts[0].blockNumber)) throw new Error("finality regressed");
    const canonicalFinal = await p.send("eth_getBlockByNumber", [final.number, false]);
    if (canonicalFinal?.hash !== final.hash) throw new Error("finalized anchor changed");
  }
  await chain(providers);
  return { tx_hash: txHash, address: expected.predicted_address, block: receipts[0].blockNumber, block_hash: receipts[0].blockHash };
}

export async function submitTestnetDeployment(planPath, pin, keyPath, journal, urls, authorized) {
  if (!authorized) throw new Error("explicit testnet transaction authorization required");
  const plan = decodePinnedPlan(await readFile(planPath), pin);
  if (plan.mode !== "prepare_only" || plan.test_only !== true || plan.audited !== false
      || !Array.isArray(plan.calls) || plan.calls.length < 5 || plan.calls.length > 6) throw new Error("invalid test-only plan");
  if (urls.length !== 2 || new URL(urls[0]).href === new URL(urls[1]).href
      || urls.some((u) => !["https:", "http:"].includes(new URL(u).protocol))) throw new Error("distinct RPCs required");
  const keyStat = await lstat(keyPath);
  if (!keyStat.isFile() || keyStat.isSymbolicLink() || keyStat.size !== 32 || (keyStat.mode & 0o077)
      || (process.getuid && keyStat.uid !== process.getuid())) throw new Error("unsafe gas key");
  const bytes = await readFile(keyPath);
  let wallet;
  try { wallet = new Wallet(`0x${bytes.toString("hex")}`); } finally { bytes.fill(0); }
  if (plan.calls.some((c, i) => c.chain_id !== 10143 || c.value !== "0"
      || c.from.toLowerCase() !== wallet.address.toLowerCase() || c.nonce !== plan.calls[0].nonce + i
      || keccak256(c.data) !== c.initcode_keccak256
      || getCreateAddress({ from: c.from, nonce: c.nonce }).toLowerCase() !== c.predicted_address.toLowerCase())) throw new Error("invalid creation intents");
  await privateDirectory(journal);
  const lock = resolve(journal, "executor.lock");
  await mkdir(lock, { mode: 0o700 });
  const providers = urls.map((u) => new JsonRpcProvider(u, undefined, { cacheTimeout: -1 }));
  try {
    const retained = await existing(resolve(journal, "plan.json"));
    if (retained && retained.sha256 !== pin) throw new Error("journal belongs to a different plan");
    if (!retained) await durableCreate(journal, "plan.json", { sha256: pin, test_only: true, audited: false });
    await chain(providers);
    for (const pool of plan.pools) {
      for (const code of await pair(providers, "eth_getCode", [pool.asset, "finalized"])) {
        if (keccak256(code).slice(2) !== pool.asset_runtime_keccak256) throw new Error("asset runtime changed");
      }
    }
    const hashes = [];
    for (const [index, call] of plan.calls.entries()) {
      await chain(providers);
      let signed = await existing(resolve(journal, `${index}-signed.json`));
      if (!signed) {
        const nonces = await pair(providers, "eth_getTransactionCount", [wallet.address, "pending"]);
        if (nonces.some((n) => BigInt(n) !== BigInt(call.nonce))) throw new Error("pending nonce changed; plan cannot be reused");
        const mined = await pair(providers, "eth_getTransactionCount", [wallet.address, "latest"]);
        if (mined.some((n) => BigInt(n) !== BigInt(call.nonce))) throw new Error("older unmined transaction holds the deployment lane");
        const gas = await providers[0].estimateGas({ from: wallet.address, data: call.data, value: 0n });
        const fees = await providers[0].getFeeData();
        if (!fees.maxFeePerGas || fees.maxPriorityFeePerGas === null) throw new Error("EIP-1559 fees unavailable");
        const gasLimit = gas * 12n / 10n + 1n;
        if (await providers[0].getBalance(wallet.address) < gasLimit * fees.maxFeePerGas) throw new Error("insufficient gas funding");
        console.log(JSON.stringify({ status: "estimated", name: call.name, gas_limit: gasLimit.toString(),
          max_cost_wei: (gasLimit * fees.maxFeePerGas).toString() }));
        const raw = await wallet.signTransaction({ type: 2, chainId: 10143, nonce: call.nonce, data: call.data,
          value: 0n, gasLimit, maxFeePerGas: fees.maxFeePerGas, maxPriorityFeePerGas: fees.maxPriorityFeePerGas });
        signed = { raw, hash: validateSignedCreation(raw, call), plan_sha256: pin };
        await durableCreate(journal, `${index}-signed.json`, signed);
      }
      if (signed.plan_sha256 !== pin || validateSignedCreation(signed.raw, call) !== signed.hash) throw new Error("retained signed bytes mismatch");
      hashes.push(signed.hash);
      let confirmed = await finalizedCreation(providers, signed.hash, call);
      const attempt = await existing(resolve(journal, `${index}-attempt.json`));
      if (attempt && (attempt.hash !== signed.hash || attempt.plan_sha256 !== pin)) throw new Error("attempt identity mismatch");
      if (!confirmed && !attempt) {
        const nonces = await pair(providers, "eth_getTransactionCount", [wallet.address, "pending"]);
        if (nonces.some((n) => BigInt(n) !== BigInt(call.nonce))) throw new Error("nonce changed before submission");
        const mined = await pair(providers, "eth_getTransactionCount", [wallet.address, "latest"]);
        if (mined.some((n) => BigInt(n) !== BigInt(call.nonce))) throw new Error("older unmined transaction holds submission");
        await durableCreate(journal, `${index}-attempt.json`, { hash: signed.hash, plan_sha256: pin });
        const returned = await providers[0].send("eth_sendRawTransaction", [signed.raw]);
        if (returned.toLowerCase() !== signed.hash.toLowerCase()) throw new Error("ambiguous broadcast hash; observe retained attempt");
      }
      const deadline = Date.now() + 180_000;
      while (!confirmed && Date.now() < deadline) {
        await new Promise((done) => setTimeout(done, 2000));
        confirmed = await finalizedCreation(providers, signed.hash, call);
      }
      if (!confirmed) throw new Error("retained attempt pending; resume observation only");
      if (!await existing(resolve(journal, `${index}-finalized.json`))) await durableCreate(journal, `${index}-finalized.json`, confirmed);
      console.log(JSON.stringify({ status: "finalized", name: call.name, ...confirmed }));
    }
    if (!await existing(resolve(journal, "hashes.json"))) await durableCreate(journal, "hashes.json", hashes);
    return hashes;
  } finally { providers.forEach((p) => p.destroy()); await rmdir(lock); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 10) throw new Error("invalid arguments");
    await submitTestnetDeployment(process.argv[2], process.argv[3], process.argv[4], resolve(process.argv[5]),
      process.argv.slice(6, 8), process.argv[8] === "--authorize-testnet-transactions" && process.argv[9] === "test-only-unaudited");
  } catch { console.error(JSON.stringify({ status: "held", error: "testnet deployment stopped; inspect retained journal without creating a new plan" })); process.exitCode = 1; }
}
