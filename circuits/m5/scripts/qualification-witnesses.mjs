// Offline synthetic witnesses for installed-prover qualification, never live wallet material.
import assert from "node:assert/strict";
import { createHash, randomBytes } from "node:crypto";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { buildEddsa } from "circomlibjs";
import * as snarkjs from "snarkjs";
import { tree } from "./pool.mjs";

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const prime = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
const json = (value) => JSON.stringify(value, (_, item) => typeof item === "bigint" ? item.toString() : item, 2);

try {
  if (process.argv.length !== 7) throw new Error("usage: deployment bundle manifest-sha256 pool-name new-output-directory");
  const [deploymentFile, bundlePath, pin, name, outputPath] = process.argv.slice(2);
  const deployment = JSON.parse(await readFile(deploymentFile, "utf8"));
  assert.equal(deployment.chain_id, 10143);
  assert.equal(deployment.test_only, true);
  assert.equal(deployment.audited, false);
  const bundle = resolve(bundlePath);
  const bytes = await readFile(resolve(bundle, "manifest.json"));
  assert.equal(sha(bytes), pin, "manifest pin mismatch");
  const manifest = JSON.parse(bytes);
  const pool = deployment.pools.find((entry) => entry.name === name);
  assert(pool, "unknown pool");
  assert.equal(manifest.test_only, true);
  assert.equal(manifest.chain_id, 10143);
  assert.equal(manifest.verifier_version, 2);
  assert.equal(manifest.settlement_contract.toLowerCase(), pool.pool.toLowerCase());

  const eddsa = await buildEddsa();
  const F = eddsa.babyJub.F;
  const hash = (values) => BigInt(F.toObject(eddsa.poseidon(values)));
  const field = () => 1n + BigInt(`0x${randomBytes(32).toString("hex")}`) % (prime - 1n);
  const limb = () => BigInt(`0x${randomBytes(16).toString("hex")}`);
  const buyers = [randomBytes(32), randomBytes(32)];
  const [buyer, seller] = buyers.map((key) => eddsa.prv2pub(key).map((value) => BigInt(F.toObject(value))));
  const common = { chainId: 10143n, contractAddress: BigInt(pool.pool), verifierVersion: 2n, asset: BigInt(pool.asset) };
  const domain = hash([common.chainId, common.contractAddress, common.verifierVersion]);
  const opening = (amount, owner, salt = field()) => {
    const secret = field();
    const tag = hash([2006n, secret]);
    const commitment = hash([2010n, domain, common.asset, amount, ...owner, tag, salt]);
    return { amount, owner, salt, secret, tag, commitment };
  };
  const input = opening(150n, buyer);
  const privateDomain = { privateChainId: common.chainId, privateContractAddress: common.contractAddress, privateVerifierVersion: 2n };
  const deposit = { ...common, ...privateDomain, amount: input.amount, noteCommitment: input.commitment,
    ownerAx: buyer[0], ownerAy: buyer[1], spendTag: input.tag, salt: input.salt };
  const nonceLo = limb(), nonceHi = limb(), dealId = limb(), transcriptLo = limb(), transcriptHi = limb();
  const serviceLo = limb(), serviceHi = limb(), blinding = field(), expiry = 4102444800n;
  const payment = opening(70n, seller, hash([2009n, domain, nonceLo, nonceHi]));
  const change = opening(80n, buyer);
  const commitment = hash([2002n, hash([2001n, domain, dealId, 1n, transcriptLo, transcriptHi]),
    hash([serviceLo, serviceHi, nonceLo, nonceHi, common.asset, payment.amount]),
    hash([...buyer, ...seller, payment.tag, payment.salt]), blinding, expiry]);
  const buyerSig = eddsa.signPoseidon(buyers[0], eddsa.poseidon([2004n, domain, commitment]));
  const sellerSig = eddsa.signPoseidon(buyers[1], eddsa.poseidon([2005n, domain, commitment]));
  const first = tree(hash, [input.commitment], 0);
  const transfer = { ...common, dealCommitment: commitment,
    dealNullifier: hash([2003n, domain, ...buyer, nonceLo, nonceHi]), root: first.root,
    inputNullifier: hash([2011n, domain, input.secret, input.commitment]),
    paymentCommitment: payment.commitment, changeCommitment: change.commitment, expiry,
    dealId, revision: 1n, transcriptRootLo: transcriptLo, transcriptRootHi: transcriptHi,
    serviceDigestLo: serviceLo, serviceDigestHi: serviceHi, settlementNonceLo: nonceLo, settlementNonceHi: nonceHi,
    amount: payment.amount, blinding, buyerAx: buyer[0], buyerAy: buyer[1], sellerAx: seller[0], sellerAy: seller[1],
    recipientSpendTag: payment.tag, buyerR8x: F.toObject(buyerSig.R8[0]), buyerR8y: F.toObject(buyerSig.R8[1]), buyerS: buyerSig.S,
    sellerR8x: F.toObject(sellerSig.R8[0]), sellerR8y: F.toObject(sellerSig.R8[1]), sellerS: sellerSig.S,
    inputAmount: input.amount, inputSalt: input.salt, inputSpendSecret: input.secret,
    pathElements: first.pathElements, pathIndices: first.pathIndices,
    changeAmount: change.amount, changeSpendTag: change.tag, changeSpendSecret: change.secret, changeSalt: change.salt };
  const last = tree(hash, [input.commitment, payment.commitment, change.commitment], 1);
  const recipient = BigInt(`0x${randomBytes(20).toString("hex")}`);
  const withdraw = { ...common, ...privateDomain, root: last.root,
    noteNullifier: hash([2011n, domain, payment.secret, payment.commitment]), recipient, amount: payment.amount,
    ownerAx: seller[0], ownerAy: seller[1], spendSecret: payment.secret, salt: payment.salt,
    pathElements: last.pathElements, pathIndices: last.pathIndices, privateRecipient: recipient };
  buyers.forEach((key) => key.fill(0));

  const output = resolve(outputPath);
  await mkdir(output, { mode: 0o700 });
  const results = [];
  for (const [kind, witness] of [["deposit", deposit], ["transfer", transfer], ["withdraw", withdraw]]) {
    const circuit = manifest.circuits.find((entry) => entry.kind === kind);
    assert(circuit, "missing circuit");
    assert.equal(circuit.identity.verifier_runtime_keccak256, pool.runtime_pins[kind]);
    const wasm = resolve(bundle, `${kind}_js/${kind}.wasm`), zkey = resolve(bundle, `${kind}.zkey`);
    const keyBytes = await readFile(resolve(bundle, `${kind}-verification-key.json`));
    assert.equal(sha(await readFile(wasm)), circuit.wasm.sha256);
    assert.equal(sha(await readFile(zkey)), circuit.zkey.sha256);
    assert.equal(sha(keyBytes), circuit.identity.verification_key_sha256);
    const inputBytes = Buffer.from(`${json(witness)}\n`);
    await writeFile(resolve(output, `${kind}-input.json`), inputBytes, { flag: "wx", mode: 0o600 });
    const proved = await snarkjs.groth16.fullProve(JSON.parse(inputBytes), wasm, zkey);
    assert(await snarkjs.groth16.verify(JSON.parse(keyBytes), proved.publicSignals, proved.proof), "invalid proof");
    assert.equal(proved.publicSignals[0], "10143");
    assert.equal(proved.publicSignals[1], common.contractAddress.toString());
    assert.equal(proved.publicSignals[2], "2");
    assert.equal(proved.publicSignals[3], common.asset.toString());
    results.push({ kind, input_sha256: sha(inputBytes), verified: true });
  }
  const report = { test_only: true, synthetic: true, on_chain_history: false, live_transactions_sent: 0,
    manifest_sha256: pin, pool: pool.pool, asset: pool.asset, results };
  await writeFile(resolve(output, "qualification.json"), `${json(report)}\n`, { flag: "wx", mode: 0o600 });
  console.log(JSON.stringify(report));
} catch (error) {
  console.error(`offline qualification failed: ${error.message}`);
  process.exitCode = 1;
} finally {
  await globalThis.curve_bn128?.terminate();
}
