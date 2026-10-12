// Local randomized conservation tests with real proofs and known-entropy test setup.
// No witness or key material is written, and no public-network RPC is accepted.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { buildEddsa, poseidonContract } from "circomlibjs";
import { ContractFactory, JsonRpcProvider } from "ethers";
import { artifactBuild, compileContracts, prove, tree } from "./pool.mjs";

const prime = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
const sha = (value) => createHash("sha256").update(value).digest("hex");
const manifest = JSON.parse(readFileSync(resolve(artifactBuild, "artifact-manifest.json")));
for (const name of ["deposit", "transfer", "withdraw"]) {
  for (const [file, hash] of [
    [`${name}.r1cs`, "r1csSha256"], [`${name}_js/${name}.wasm`, "wasmSha256"],
    [`${name}.zkey`, "zkeySha256"], [`${name}-verification-key.json`, "verificationKeySha256"],
    [`${name}-verifier.sol`, "verifierSourceSha256"],
  ]) assert.equal(sha(readFileSync(resolve(artifactBuild, file))), manifest.circuits[name][hash]);
}

const server = createServer();
await new Promise((ready, reject) => server.once("error", reject).listen(0, "127.0.0.1", ready));
const port = server.address().port;
await new Promise((done) => server.close(done));
const anvil = spawn("anvil", ["--port", String(port), "--chain-id", "31337", "--silent"], { stdio: "ignore" });
const provider = new JsonRpcProvider(`http://127.0.0.1:${port}`, 31337, { staticNetwork: true, cacheTimeout: -1 });
provider.pollingInterval = 50;
try {
  let ready = false;
  for (let i = 0; i < 80; i += 1) {
    try { await provider.getBlockNumber(); ready = true; break; } catch { await delay(100); }
  }
  assert(ready, "local Anvil failed to start");
  const signer = await provider.getSigner(0);
  const payout = await (await provider.getSigner(1)).getAddress();
  const eddsa = await buildEddsa();
  const F = eddsa.babyJub.F;
  const hash = (values) => F.toObject(eddsa.poseidon(values));
  const contracts = compileContracts();
  const deploy = async (artifact, ...args) => {
    const contract = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object, signer).deploy(...args);
    await contract.waitForDeployment();
    return contract;
  };
  const verifiers = {};
  for (const name of ["deposit", "transfer", "withdraw"]) {
    verifiers[name] = await deploy(contracts[`${name}-verifier.sol`].Groth16Verifier);
  }
  const hasher = await new ContractFactory(poseidonContract.generateABI(2), poseidonContract.createCode(2), signer).deploy();
  await hasher.waitForDeployment();

  for (const seed of ["stateful-v2-0", "stateful-v2-1", "stateful-v2-2"]) {
    let counter = 0;
    const random = () => BigInt(`0x${sha(`${seed}:${counter++}`)}`);
    const field = () => 1n + random() % (prime - 1n);
    const owners = [0, 1].map((index) => {
      const secret = Buffer.from(sha(`${seed}:owner:${index}`), "hex");
      return { secret, point: eddsa.prv2pub(secret).map((value) => F.toObject(value)) };
    });
    const token = await deploy(contracts["MockERC20.sol"].MockERC20, "Stateful Test", "STATE");
    const pool = await deploy(contracts["ErebusShieldedPool.sol"].ErebusShieldedPool,
      await token.getAddress(), await hasher.getAddress(), await verifiers.deposit.getAddress(),
      await verifiers.transfer.getAddress(), await verifiers.withdraw.getAddress(), 2);
    const chainId = 31337n;
    const contractAddress = BigInt(await pool.getAddress());
    const asset = BigInt(await token.getAddress());
    const domain = hash([chainId, contractAddress, 2n]);
    const common = { chainId, contractAddress, verifierVersion: 2n, asset };
    const leaves = [];
    const notes = [];
    const spent = [];
    const deals = [];
    let deposited = 0n;
    let withdrawn = 0n;
    let proofs = 0;
    const opening = (amount, owner, salt = field(), secret = field()) => {
      const tag = hash([2006n, secret]);
      const commitment = hash([2010n, domain, asset, amount, ...owners[owner].point, tag, salt]);
      return { amount, owner, salt, secret, tag, commitment, index: leaves.length, spent: false };
    };
    const append = (note) => {
      note.index = leaves.length;
      leaves.push(note.commitment);
      notes.push(note);
    };
    const check = async () => {
      assert.equal(await pool.currentRoot(), tree(hash, leaves, 0).root);
      assert.equal(await pool.nextLeafIndex(), BigInt(leaves.length));
      assert.equal(await token.balanceOf(await pool.getAddress()), deposited - withdrawn);
      assert.equal(notes.filter((note) => !note.spent).reduce((sum, note) => sum + note.amount, 0n), deposited - withdrawn);
      assert.equal(await token.balanceOf(payout), withdrawn);
      assert.equal(await token.totalSupply(), deposited);
      for (const nullifier of spent) assert(await pool.consumedNotes(nullifier));
      for (const nullifier of deals) assert(await pool.consumedDeals(nullifier));
    };
    const reject = async (method, calldata) => {
      await assert.rejects(async () => {
        const tx = await pool[method](...calldata, { gasLimit: 5000000 });
        await tx.wait();
      });
      await check();
    };

    for (let owner = 0; owner < 2; owner += 1) {
      const amount = 100n + random() % 400n;
      const note = opening(amount, owner);
      const input = { ...common, amount, noteCommitment: note.commitment,
        ownerAx: owners[owner].point[0], ownerAy: owners[owner].point[1], spendTag: note.tag, salt: note.salt,
        privateChainId: chainId, privateContractAddress: contractAddress, privateVerifierVersion: 2n };
      const proof = await prove("deposit", input, [chainId, contractAddress, 2n, asset, amount, note.commitment]);
      proofs += 1;
      await (await token.mint(await signer.getAddress(), amount)).wait();
      await (await token.approve(await pool.getAddress(), amount)).wait();
      await (await pool.deposit(...proof)).wait();
      deposited += amount;
      append(note);
      await check();
      await reject("deposit", proof);
    }

    let priorChange;
    for (let step = 0; step < 4; step += 1) {
      const available = notes.filter((note) => !note.spent && note.amount > 0n);
      const note = step % 2 === 1 && priorChange?.amount > 0n
        ? priorChange : available[Number(random() % BigInt(available.length))];
      const recipient = 1 - note.owner;
      const amount = step === 2 ? note.amount : 1n + random() % note.amount;
      const path = tree(hash, leaves, note.index);
      const nonceLo = random() % (1n << 128n);
      const nonceHi = random() % (1n << 128n);
      const payment = opening(amount, recipient, hash([2009n, domain, nonceLo, nonceHi]));
      const change = opening(note.amount - amount, note.owner);
      const dealId = random() % (1n << 128n);
      const transcriptLo = random() % (1n << 128n);
      const transcriptHi = random() % (1n << 128n);
      const serviceLo = random() % (1n << 128n);
      const serviceHi = random() % (1n << 128n);
      const blinding = field();
      const expiry = 4102444800n;
      const buyer = owners[note.owner];
      const seller = owners[recipient];
      const commitment = hash([2002n,
        hash([2001n, domain, dealId, 1n, transcriptLo, transcriptHi]),
        hash([serviceLo, serviceHi, nonceLo, nonceHi, asset, amount]),
        hash([...buyer.point, ...seller.point, payment.tag, payment.salt]), blinding, expiry]);
      const dealNullifier = hash([2003n, domain, ...buyer.point, nonceLo, nonceHi]);
      const nullifier = hash([2011n, domain, note.secret, note.commitment]);
      const buyerSig = eddsa.signPoseidon(buyer.secret, eddsa.poseidon([2004n, domain, commitment]));
      const sellerSig = eddsa.signPoseidon(seller.secret, eddsa.poseidon([2005n, domain, commitment]));
      const input = { ...common, dealCommitment: commitment, dealNullifier, root: path.root,
        inputNullifier: nullifier, paymentCommitment: payment.commitment, changeCommitment: change.commitment, expiry,
        dealId, revision: 1n, transcriptRootLo: transcriptLo, transcriptRootHi: transcriptHi,
        serviceDigestLo: serviceLo, serviceDigestHi: serviceHi, settlementNonceLo: nonceLo, settlementNonceHi: nonceHi,
        amount, blinding, buyerAx: buyer.point[0], buyerAy: buyer.point[1], sellerAx: seller.point[0], sellerAy: seller.point[1],
        recipientSpendTag: payment.tag, buyerR8x: F.toObject(buyerSig.R8[0]), buyerR8y: F.toObject(buyerSig.R8[1]), buyerS: buyerSig.S,
        sellerR8x: F.toObject(sellerSig.R8[0]), sellerR8y: F.toObject(sellerSig.R8[1]), sellerS: sellerSig.S,
        inputAmount: note.amount, inputSalt: note.salt, inputSpendSecret: note.secret,
        pathElements: path.pathElements, pathIndices: path.pathIndices,
        changeAmount: change.amount, changeSpendTag: change.tag, changeSpendSecret: change.secret, changeSalt: change.salt };
      const signals = [chainId, contractAddress, 2n, asset, commitment, dealNullifier, path.root,
        nullifier, payment.commitment, change.commitment, expiry];
      const proof = await prove("transfer", input, signals);
      proofs += 1;
      const altered = structuredClone(proof);
      altered[3][8] = String((payment.commitment + 1n) % prime);
      await reject("transferPrivate", altered);
      assert.equal(await pool.consumedNotes(nullifier), false);
      assert.equal(await pool.consumedDeals(dealNullifier), false);
      await (await pool.transferPrivate(...proof)).wait();
      note.spent = true;
      spent.push(nullifier);
      deals.push(dealNullifier);
      append(payment);
      append(change);
      priorChange = change;
      await check();
      await reject("transferPrivate", proof);
    }

    for (const note of notes.filter((candidate) => !candidate.spent && candidate.amount > 0n)) {
      const path = tree(hash, leaves, note.index);
      const nullifier = hash([2011n, domain, note.secret, note.commitment]);
      const input = { ...common, root: path.root, noteNullifier: nullifier, recipient: BigInt(payout), amount: note.amount,
        ownerAx: owners[note.owner].point[0], ownerAy: owners[note.owner].point[1], spendSecret: note.secret, salt: note.salt,
        pathElements: path.pathElements, pathIndices: path.pathIndices, privateChainId: chainId,
        privateContractAddress: contractAddress, privateVerifierVersion: 2n, privateRecipient: BigInt(payout) };
      const proof = await prove("withdraw", input, [chainId, contractAddress, 2n, asset, path.root, nullifier, BigInt(payout), note.amount]);
      proofs += 1;
      const altered = structuredClone(proof);
      altered[3][7] = String(note.amount + 1n);
      await reject("withdraw", altered);
      assert.equal(await pool.consumedNotes(nullifier), false);
      await (await pool.withdraw(...proof)).wait();
      note.spent = true;
      spent.push(nullifier);
      withdrawn += note.amount;
      await check();
      await reject("withdraw", proof);
    }
    assert.equal(deposited, withdrawn);
    assert.equal(await token.balanceOf(await pool.getAddress()), 0n);
    console.log(JSON.stringify({ seed, proofs, deposits: 2, transfers: 4, deposited: String(deposited),
      withdrawn: String(withdrawn), leaves: leaves.length, test_only: true, live_network: false }));
  }
} finally {
  await provider.destroy();
  if (anvil.exitCode === null) {
    anvil.kill("SIGTERM");
    await Promise.race([once(anvil, "exit"), delay(1000)]);
  }
  await globalThis.curve_bn128?.terminate();
}
