// Reconstruct already-public deployment bytes. Never signs or broadcasts.
import { readFile, mkdir, open } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { Transaction, getCreateAddress, keccak256 } from "ethers";

const HASH = /^0x[0-9a-f]{64}$/;
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const QUANTITY = /^0x(?:0|[1-9a-f][0-9a-f]*)$/;
const METHODS = new Set(["eth_chainId", "eth_getBlockByNumber", "eth_getTransactionByHash",
  "eth_getTransactionReceipt", "eth_getCode"]);
const NAMES = ["poseidon", "deposit", "transfer", "withdraw", "pool_wmon", "pool_usdc"];
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
function require(value) { if (!value) throw new Error("deployment replay evidence unavailable or conflicting"); }
function quantity(value) { require(typeof value === "string" && QUANTITY.test(value)); return BigInt(value); }
function block(value) {
  require(value && HASH.test(value.hash));
  return { number: quantity(value.number), hash: value.hash };
}

export function reconstructCreation(tx, receipt, creation) {
  require(tx && receipt && tx.type === "0x2" && tx.to === null && quantity(tx.chainId) === 10143n
    && quantity(tx.value) === 0n && ADDRESS.test(tx.from) && HASH.test(tx.hash)
    && tx.hash === creation.tx_hash && receipt.transactionHash === tx.hash
    && receipt.status === "0x1" && receipt.to === null && receipt.from?.toLowerCase() === tx.from.toLowerCase()
    && receipt.contractAddress?.toLowerCase() === creation.address.toLowerCase()
    && tx.blockHash === creation.block_hash && receipt.blockHash === creation.block_hash
    && quantity(tx.blockNumber) === BigInt(creation.block) && quantity(receipt.blockNumber) === BigInt(creation.block));
  const nonce = quantity(tx.nonce);
  require(nonce <= BigInt(Number.MAX_SAFE_INTEGER));
  const decoded = Transaction.from({ type: 2, chainId: 10143n, nonce: Number(nonce), to: null,
    value: 0n, gasLimit: quantity(tx.gas), maxFeePerGas: quantity(tx.maxFeePerGas),
    maxPriorityFeePerGas: quantity(tx.maxPriorityFeePerGas), data: tx.input,
    accessList: tx.accessList, signature: { r: tx.r, s: tx.s, v: Number(quantity(tx.v)) } });
  require(decoded.isSigned() && decoded.hash === tx.hash && decoded.from.toLowerCase() === tx.from.toLowerCase()
    && getCreateAddress({ from: decoded.from, nonce: decoded.nonce }).toLowerCase() === creation.address.toLowerCase());
  return { raw: decoded.serialized, hash: decoded.hash, nonce: decoded.nonce };
}

export async function collectReplay(record, send) {
  require(record?.chain_id === 10143 && Array.isArray(record.creations) && record.creations.length === NAMES.length);
  for (const [i, creation] of record.creations.entries()) {
    require(creation.name === NAMES[i] && ADDRESS.test(creation.address) && HASH.test(creation.tx_hash)
      && HASH.test(creation.block_hash) && HASH.test(creation.runtime_keccak256)
      && Number.isSafeInteger(creation.block) && creation.block > 0);
  }
  const tips = [];
  for (let peer = 0; peer < 2; peer++) {
    require(quantity(await send(peer, "eth_chainId", [])) === 10143n);
    tips.push(block(await send(peer, "eth_getBlockByNumber", ["finalized", false])));
  }
  const signed = [];
  for (const creation of record.creations) {
    const copies = [];
    for (let peer = 0; peer < 2; peer++) {
      require(tips[peer].number >= BigInt(creation.block));
      const canonical = block(await send(peer, "eth_getBlockByNumber", [`0x${creation.block.toString(16)}`, false]));
      require(canonical.number === BigInt(creation.block) && canonical.hash === creation.block_hash);
      const tx = await send(peer, "eth_getTransactionByHash", [creation.tx_hash]);
      const receipt = await send(peer, "eth_getTransactionReceipt", [creation.tx_hash]);
      copies.push(reconstructCreation(tx, receipt, creation));
      const code = await send(peer, "eth_getCode", [creation.address,
        { blockHash: creation.block_hash, requireCanonical: true }]);
      require(typeof code === "string" && /^0x(?:[0-9a-fA-F]{2})+$/.test(code)
        && keccak256(code) === creation.runtime_keccak256);
    }
    require(copies[0].raw === copies[1].raw);
    signed.push(copies[0]);
  }
  require(signed.every((tx, i) => i === 0 || tx.nonce === signed[i - 1].nonce + 1));
  require(record.creations.every((creation, i) => getCreateAddress({
    from: Transaction.from(signed[0].raw).from, nonce: signed[i].nonce,
  }).toLowerCase() === creation.address.toLowerCase()));
  for (let peer = 0; peer < 2; peer++) {
    for (const anchor of [...record.creations.map((creation) => ({ number: BigInt(creation.block), hash: creation.block_hash })), tips[peer]]) {
      const again = block(await send(peer, "eth_getBlockByNumber", [`0x${anchor.number.toString(16)}`, false]));
      require(again.number === anchor.number && again.hash === anchor.hash);
    }
    require(block(await send(peer, "eth_getBlockByNumber", ["finalized", false])).number >= tips[peer].number);
    require(quantity(await send(peer, "eth_chainId", [])) === 10143n);
  }
  return { signed, anchors: tips.map((tip) => ({ block: tip.number.toString(), hash: tip.hash })) };
}

export function parseOptions(args) {
  const allowed = new Set(["--deployment", "--deployment-sha256", "--rpc-url", "--peer-rpc-url", "--output"]);
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    require(allowed.has(args[i]) && !Object.hasOwn(options, args[i]) && args[i + 1]);
    options[args[i]] = args[i + 1];
  }
  require(Object.keys(options).length === allowed.size && /^[0-9a-f]{64}$/.test(options["--deployment-sha256"]));
  const urls = [options["--rpc-url"], options["--peer-rpc-url"]].map((value) => new URL(value));
  require(urls.every((url) => url.protocol === "https:" && !url.username && !url.password && !url.hash)
    && urls[0].href !== urls[1].href);
  return { deployment: resolve(options["--deployment"]), pin: options["--deployment-sha256"],
    urls: urls.map((url) => url.href), output: resolve(options["--output"]) };
}

async function write(directory, name, value) {
  const file = await open(resolve(directory, name), "wx", 0o600);
  try { await file.writeFile(`${JSON.stringify(value)}\n`); await file.sync(); } finally { await file.close(); }
}
async function main() {
  try {
    const options = parseOptions(process.argv.slice(2));
    const bytes = await readFile(options.deployment);
    require(bytes.length <= 1 << 20 && sha256(bytes) === options.pin);
    const record = JSON.parse(bytes);
    const send = async (peer, method, params) => {
      require(METHODS.has(method));
      const response = await fetch(options.urls[peer], { method: "POST", redirect: "error",
        headers: { "content-type": "application/json", "user-agent": "erebus-replay-export/1" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }), signal: AbortSignal.timeout(30_000) });
      require(response.ok);
      const reader = response.body.getReader();
      const chunks = [];
      let size = 0;
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          size += value.length;
          require(size <= 1 << 20);
          chunks.push(value);
        }
      } finally { await reader.cancel(); }
      const reply = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      require(reply.jsonrpc === "2.0" && reply.id === 1 && !reply.error && Object.hasOwn(reply, "result"));
      return reply.result;
    };
    const evidence = await collectReplay(record, send);
    await mkdir(options.output, { mode: 0o700 });
    for (const [i, tx] of evidence.signed.entries()) await write(options.output, `${i}-signed.json`, tx);
    const report = { version: 1, deployment_sha256: options.pin, chain_id: 10143,
      transaction_hashes: evidence.signed.map((tx) => tx.hash), anchors: evidence.anchors,
      read_only: true, signed_or_broadcast: false, purpose: "local-replay-of-public-deployment" };
    await write(options.output, "replay.json", report);
    const directory = await open(options.output, "r");
    try { await directory.sync(); } finally { await directory.close(); }
    console.log(JSON.stringify({ status: "verified", ...report }));
  } catch { console.error("public deployment replay export failed; no transaction sent; retain any partial output"); process.exitCode = 1; }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
