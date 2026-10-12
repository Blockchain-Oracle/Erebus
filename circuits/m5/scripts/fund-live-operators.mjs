// Fixed-size testnet gas funding for the prepared participants; exact retained bytes only.
import { readFile, mkdir, open, lstat, rmdir } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";
import { Wallet, Transaction, JsonRpcProvider } from "ethers";

export function validateSignedFunding(raw, sender, recipient, nonce) {
  const tx = Transaction.from(raw);
  if (!tx.isSigned() || tx.type !== 2 || tx.chainId !== 10143n || tx.value !== 2_000_000_000_000_000_000n
      || tx.from.toLowerCase() !== sender.toLowerCase() || tx.to?.toLowerCase() !== recipient.toLowerCase()
      || tx.data !== "0x" || tx.nonce !== nonce) throw new Error("retained funding intent mismatch");
  return tx.hash;
}
async function write(directory, name, value) {
  const file = await open(resolve(directory, name), "wx", 0o600);
  try { await file.writeFile(`${JSON.stringify(value)}\n`); await file.sync(); } finally { await file.close(); }
  const parent = await open(directory, "r");
  try { await parent.sync(); } finally { await parent.close(); }
}
async function record(directory, name) {
  try {
    const path = resolve(directory, name);
    const stat = await lstat(path);
    if (!stat.isFile() || stat.isSymbolicLink() || (stat.mode & 0o077)
        || stat.uid !== process.getuid()) throw new Error("unsafe funding record");
    return JSON.parse(await readFile(path, "utf8"));
  } catch (error) { if (error.code === "ENOENT") return null; throw error; }
}
async function pair(providers, method, params) {
  const values = [];
  for (const p of providers) values.push(await p.send(method, params));
  return values;
}
async function checkChain(providers) {
  if (!(await pair(providers, "eth_chainId", [])).every((n) => BigInt(n) === 10143n)) throw new Error("wrong chain");
}
async function finalized(providers, hash, sender, recipient, nonce) {
  const receipts = await pair(providers, "eth_getTransactionReceipt", [hash]);
  if (receipts.some((r) => !r)) return null;
  const tips = await pair(providers, "eth_getBlockByNumber", ["finalized", false]);
  if (tips.some((b) => !b?.number || !b.hash)) throw new Error("finality unavailable");
  if (receipts.some((r, i) => BigInt(r.blockNumber) > BigInt(tips[i].number))) return null;
  if (receipts[0].blockHash !== receipts[1].blockHash || receipts[0].blockNumber !== receipts[1].blockNumber) throw new Error("receipts conflict");
  for (const [i, p] of providers.entries()) {
    const r = receipts[i];
    if (BigInt(r.status) !== 1n || r.transactionHash.toLowerCase() !== hash.toLowerCase()
        || r.from.toLowerCase() !== sender.toLowerCase() || r.to?.toLowerCase() !== recipient.toLowerCase()) throw new Error("funding receipt mismatch");
    for (const anchor of [tips[i], { number: r.blockNumber, hash: r.blockHash }]) {
      const canonical = await p.send("eth_getBlockByNumber", [anchor.number, false]);
      if (canonical?.hash !== anchor.hash) throw new Error("noncanonical funding evidence");
    }
    const tx = await p.send("eth_getTransactionByHash", [hash]);
    if (!tx || tx.from.toLowerCase() !== sender.toLowerCase() || tx.to?.toLowerCase() !== recipient.toLowerCase()
        || BigInt(tx.chainId) !== 10143n || BigInt(tx.nonce) !== BigInt(nonce)
        || BigInt(tx.value) !== 2_000_000_000_000_000_000n || tx.input !== "0x"
        || tx.blockHash !== r.blockHash || tx.hash.toLowerCase() !== hash.toLowerCase()) throw new Error("funding transaction mismatch");
  }
  await checkChain(providers);
  return { hash, recipient, block: receipts[0].blockNumber, block_hash: receipts[0].blockHash };
}

export async function fundOperators(runFile, keyFile, journalArg, authorize) {
  if (!authorize) throw new Error("explicit testnet funding authorization required");
  const bytes = await readFile(runFile);
  const pin = createHash("sha256").update(bytes).digest("hex");
  const run = JSON.parse(bytes);
  if (run.chain_id !== 10143 || run.test_only !== true || run.audited !== false) throw new Error("not a test-only testnet run");
  const recipients = [run.gas_addresses.buyer, run.gas_addresses.seller];
  if (recipients.some((a) => !/^0x[0-9a-f]{40}$/.test(a) || BigInt(a) === 0n) || recipients[0] === recipients[1]) throw new Error("invalid participants");
  const keyStat = await lstat(keyFile);
  if (!keyStat.isFile() || keyStat.isSymbolicLink() || keyStat.size !== 32 || (keyStat.mode & 0o077)
      || keyStat.uid !== process.getuid()) throw new Error("unsafe funding key");
  const seed = await readFile(keyFile);
  let wallet;
  try { wallet = new Wallet(`0x${seed.toString("hex")}`); } finally { seed.fill(0); }
  const journal = resolve(journalArg);
  await mkdir(journal, { mode: 0o700, recursive: true });
  const stat = await lstat(journal);
  if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o077) || stat.uid !== process.getuid()) throw new Error("unsafe funding journal");
  const lock = resolve(journal, "executor.lock");
  await mkdir(lock, { mode: 0o700 });
  const providers = ["https://testnet-rpc.monad.xyz", "https://rpc.ankr.com/monad_testnet"].map((u) => new JsonRpcProvider(u, undefined, { cacheTimeout: -1 }));
  try {
    await checkChain(providers);
    const old = await record(journal, "intent.json");
    if (old && (old.run_sha256 !== pin || old.sender !== wallet.address)) throw new Error("funding journal belongs to another intent");
    if (!old) await write(journal, "intent.json", { run_sha256: pin, sender: wallet.address, recipients, amount_wei: "2000000000000000000" });
    for (const [i, recipient] of recipients.entries()) {
      let signed = await record(journal, `${i}-signed.json`);
      if (!signed) {
        const nonces = await pair(providers, "eth_getTransactionCount", [wallet.address, "pending"]);
        if (nonces[0] !== nonces[1]) throw new Error("nonce disagreement");
        const mined = await pair(providers, "eth_getTransactionCount", [wallet.address, "latest"]);
        if (mined.some((n) => BigInt(n) !== BigInt(nonces[0]))) throw new Error("older unmined transaction holds funding lane");
        const nonce = Number(BigInt(nonces[0]));
        const gas = await providers[0].estimateGas({ from: wallet.address, to: recipient, value: 2_000_000_000_000_000_000n });
        const fees = await providers[0].getFeeData();
        if (!fees.maxFeePerGas || fees.maxPriorityFeePerGas === null) throw new Error("fees unavailable");
        const gasLimit = gas * 12n / 10n + 1n;
        if (await providers[0].getBalance(wallet.address) < 2_000_000_000_000_000_000n + gasLimit * fees.maxFeePerGas) throw new Error("funding balance insufficient");
        const raw = await wallet.signTransaction({ type: 2, chainId: 10143, nonce, to: recipient, value: 2_000_000_000_000_000_000n,
          data: "0x", gasLimit, maxFeePerGas: fees.maxFeePerGas, maxPriorityFeePerGas: fees.maxPriorityFeePerGas });
        signed = { raw, nonce, hash: validateSignedFunding(raw, wallet.address, recipient, nonce) };
        await write(journal, `${i}-signed.json`, signed);
      }
      if (validateSignedFunding(signed.raw, wallet.address, recipient, signed.nonce) !== signed.hash) throw new Error("retained signed hash mismatch");
      let evidence = await finalized(providers, signed.hash, wallet.address, recipient, signed.nonce);
      const attempt = await record(journal, `${i}-attempt.json`);
      if (attempt && attempt.hash !== signed.hash) throw new Error("funding attempt mismatch");
      if (!evidence && !attempt) {
        const nonces = await pair(providers, "eth_getTransactionCount", [wallet.address, "pending"]);
        if (nonces.some((n) => BigInt(n) !== BigInt(signed.nonce))) throw new Error("funding nonce changed");
        const mined = await pair(providers, "eth_getTransactionCount", [wallet.address, "latest"]);
        if (mined.some((n) => BigInt(n) !== BigInt(signed.nonce))) throw new Error("older unmined transaction holds funding submission");
        await write(journal, `${i}-attempt.json`, { hash: signed.hash });
        const returned = await providers[0].send("eth_sendRawTransaction", [signed.raw]);
        if (returned.toLowerCase() !== signed.hash.toLowerCase()) throw new Error("ambiguous funding outcome");
      }
      const deadline = Date.now() + 180_000;
      while (!evidence && Date.now() < deadline) {
        await new Promise((done) => setTimeout(done, 2000));
        evidence = await finalized(providers, signed.hash, wallet.address, recipient, signed.nonce);
      }
      if (!evidence) throw new Error("funding attempt retained; observe only");
      if (!await record(journal, `${i}-finalized.json`)) await write(journal, `${i}-finalized.json`, evidence);
      console.log(JSON.stringify({ status: "funded_finalized", ...evidence }));
    }
  } finally { providers.forEach((p) => p.destroy()); await rmdir(lock); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 6) throw new Error("invalid arguments");
    await fundOperators(process.argv[2], process.argv[3], process.argv[4], process.argv[5] === "--authorize-testnet-funding");
  } catch { console.error("funding held; retain journal; do not create another funding intent"); process.exitCode = 1; }
}
