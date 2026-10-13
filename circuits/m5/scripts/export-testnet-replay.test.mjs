import assert from "node:assert/strict";
import test from "node:test";
import { Wallet, Transaction, getCreateAddress, keccak256 } from "ethers";
import { collectReplay, parseOptions, reconstructCreation } from "./export-testnet-replay.mjs";

const wallet = new Wallet(`0x${"37".repeat(32)}`); // Local test vector, never a live signer.
const names = ["poseidon", "deposit", "transfer", "withdraw", "pool_wmon", "pool_usdc"];
const hex = (number) => `0x${BigInt(number).toString(16)}`;
const hash = (number) => `0x${BigInt(number).toString(16).padStart(64, "0")}`;
const txs = [];
const receipts = [];
const raws = [];
const record = { chain_id: 10143, creations: [] };
for (let i = 0; i < names.length; i++) {
  const raw = await wallet.signTransaction({ type: 2, chainId: 10143, nonce: i, to: null, data: "0x60006000",
    value: 0, gasLimit: 100000, maxFeePerGas: 300, maxPriorityFeePerGas: 2, accessList: [] });
  const tx = Transaction.from(raw);
  const creation = { name: names[i], address: getCreateAddress({ from: wallet.address, nonce: i }),
    tx_hash: tx.hash, block: 100 + i, block_hash: hash(100 + i), runtime_keccak256: keccak256("0x6000") };
  record.creations.push(creation);
  txs.push({ type: "0x2", chainId: hex(10143), nonce: hex(i), from: wallet.address, to: null,
    value: "0x0", gas: hex(100000), maxFeePerGas: hex(300), maxPriorityFeePerGas: hex(2), input: tx.data,
    accessList: [], r: tx.signature.r, s: tx.signature.s, v: hex(tx.signature.yParity),
    hash: tx.hash, blockNumber: hex(100 + i), blockHash: creation.block_hash });
  receipts.push({ transactionHash: tx.hash, status: "0x1", from: wallet.address, to: null,
    contractAddress: creation.address, blockNumber: hex(100 + i), blockHash: creation.block_hash });
  raws.push(raw);
}

function provider(mutate = (value) => value) {
  const calls = [];
  const send = async (peer, method, params) => {
    calls.push({ peer, method, params });
    let value;
    switch (method) {
      case "eth_chainId": value = hex(10143); break;
      case "eth_getBlockByNumber": {
        const height = params[0] === "finalized" ? 200 - peer : Number(BigInt(params[0]));
        value = { number: hex(height), hash: hash(height) }; break;
      }
      case "eth_getTransactionByHash": value = txs.find((tx) => tx.hash === params[0]); break;
      case "eth_getTransactionReceipt": value = receipts.find((receipt) => receipt.transactionHash === params[0]); break;
      case "eth_getCode":
        assert.equal(params[1].requireCanonical, true);
        assert.equal(params[1].blockHash, record.creations.find((creation) => creation.address === params[0]).block_hash);
        value = "0x6000"; break;
      default: assert.fail(`write or unexpected RPC: ${method}`);
    }
    return mutate(structuredClone(value), peer, method, params, calls);
  };
  return { send, calls };
}

test("public signatures reconstruct byte-exactly without a key", () => {
  for (let i = 0; i < raws.length; i++) assert.equal(reconstructCreation(txs[i], receipts[i], record.creations[i]).raw, raws[i]);
});
test("paired finalized export tolerates honest tip lag and sends only reads", async () => {
  const rpc = provider();
  const result = await collectReplay(record, rpc.send);
  assert.deepEqual(result.signed.map((tx) => tx.raw), raws);
  assert.equal(result.anchors[0].block, "200");
  assert.equal(result.anchors[1].block, "199");
  assert.ok(rpc.calls.length > 50);
});
for (const [name, mutate] of [
  ["receipt revert", (value, peer, method) => peer === 1 && method === "eth_getTransactionReceipt" ? { ...value, status: "0x0" } : value],
  ["changed signed calldata", (value, peer, method) => peer === 1 && method === "eth_getTransactionByHash" ? { ...value, input: "0x6001" } : value],
  ["wrong sender", (value, peer, method) => peer === 1 && method === "eth_getTransactionByHash" ? { ...value, from: "0x" + "11".repeat(20) } : value],
  ["missing transaction", (value, peer, method) => peer === 1 && method === "eth_getTransactionByHash" ? null : value],
  ["conflicting runtime", (value, peer, method) => peer === 1 && method === "eth_getCode" ? "0x6001" : value],
  ["wrong chain", (value, peer, method) => peer === 1 && method === "eth_chainId" ? "0x1" : value],
  ["nonfinalized creation", (value, peer, method, params) => peer === 1 && method === "eth_getBlockByNumber" && params[0] === "finalized" ? { number: "0x1", hash: hash(1) } : value],
  ["canonical anchor conflict", (value, peer, method, params) => peer === 1 && method === "eth_getBlockByNumber" && params[0] === "0x64" ? { ...value, hash: hash(999) } : value],
  ["anchor mutation after transaction reads", (value, peer, method, params, calls) => peer === 0 && method === "eth_getBlockByNumber" && params[0] === "0x64"
    && calls.filter((call) => call.peer === peer && call.method === method && call.params[0] === "0x64").length > 1 ? { ...value, hash: hash(999) } : value],
  ["finality regression", (value, peer, method, params, calls) => peer === 0 && method === "eth_getBlockByNumber" && params[0] === "finalized"
    && calls.filter((call) => call.peer === peer && call.method === method && call.params[0] === "finalized").length > 1 ? { number: hex(199), hash: hash(199) } : value],
]) {
  test(`${name} fails closed`, async () => { await assert.rejects(collectReplay(record, provider(mutate).send)); });
}
test("deployment record and receipt must bind the creation", async () => {
  await assert.rejects(collectReplay({ ...record, chain_id: 1 }, provider().send));
  await assert.rejects(collectReplay({ ...record, creations: record.creations.slice(1) }, provider().send));
  assert.throws(() => reconstructCreation(txs[0], { ...receipts[0], blockHash: hash(8) }, record.creations[0]));
  assert.throws(() => reconstructCreation({ ...txs[0], value: "0x1" }, receipts[0], record.creations[0]));
});
test("export options require pinned records and distinct HTTPS endpoints", () => {
  const args = ["--deployment", "record.json", "--deployment-sha256", "1".repeat(64),
    "--rpc-url", "https://primary.invalid/", "--peer-rpc-url", "https://peer.invalid/", "--output", "new"];
  assert.equal(parseOptions(args).urls.length, 2);
  for (const patch of [args.slice(0, -2), [...args, "--output", "other"], args.map((value) => value.replace("https://peer.invalid/", "https://primary.invalid/")),
    args.map((value) => value.replace("https://primary.invalid/", "http://primary.invalid/"))]) assert.throws(() => parseOptions(patch));
});
