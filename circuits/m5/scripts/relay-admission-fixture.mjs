// Known-entropy, local Anvil admission tests only. Never accepts a live RPC.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { buildEddsa } from "circomlibjs";
import { Contract, JsonRpcProvider } from "ethers";
import { artifactBuild, compileContracts, prove, tree } from "./pool.mjs";

const url = process.argv[2];
if (process.argv.length > 4 || !/^http:\/\/127\.0\.0\.1:\d+$/.test(url ?? "")) {
  throw new Error("relay admission fixture requires local Anvil");
}
const chain = process.argv[3] === undefined ? 31337n : BigInt(process.argv[3]);
if (![31337n, 10143n].includes(chain)) throw new Error("wrong fixture chain");
const provider = new JsonRpcProvider(url);
try {
  if ((await provider.getNetwork()).chainId !== chain) throw new Error("wrong fixture chain");
  const fixture = JSON.parse(execFileSync(process.execPath,
    [resolve(import.meta.dirname, "native-fixture.mjs"), url, "erc20", chain.toString()], { encoding: "utf8" }));
  const signer = await provider.getSigner(0);
  const sponsor = await provider.getSigner(1);
  const contracts = compileContracts();
  const pool = new Contract(fixture.pool, contracts["ErebusShieldedPool.sol"].ErebusShieldedPool.abi, signer);
  const token = new Contract(fixture.token, contracts["MockERC20.sol"].MockERC20.abi, signer);
  const eddsa = await buildEddsa();
  const F = eddsa.babyJub.F;
  const poseidon = (values) => BigInt(F.toObject(eddsa.poseidon(values)));
  const buyerPrivate = Buffer.alloc(32, 61);
  const sellerPrivate = Buffer.alloc(32, 62);
  const buyer = eddsa.prv2pub(buyerPrivate).map((value) => BigInt(F.toObject(value)));
  const seller = eddsa.prv2pub(sellerPrivate).map((value) => BigInt(F.toObject(value)));
  const chainId = chain, contractAddress = BigInt(fixture.pool), verifierVersion = 2n;
  const asset = BigInt(fixture.token), inputAmount = 150n, amount = 70n, changeAmount = 80n;
  const domain = poseidon([chainId, contractAddress, verifierVersion]);
  const inputSpendSecret = BigInt(`0x${"06".repeat(32)}`);
  const inputSpendTag = poseidon([2006n, inputSpendSecret]);
  const inputSalt = BigInt(`0x${"07".repeat(32)}`);
  const note = (salt) => poseidon([2010n, domain, asset, inputAmount, ...buyer, inputSpendTag, salt]);
  const firstNote = note(inputSalt), secondSalt = inputSalt + 1n, secondNote = note(secondSalt);
  const depositInput = {
    chainId, contractAddress, verifierVersion, asset, amount: inputAmount, noteCommitment: secondNote,
    ownerAx: buyer[0], ownerAy: buyer[1], spendTag: inputSpendTag, salt: secondSalt,
    privateChainId: chainId, privateContractAddress: contractAddress, privateVerifierVersion: verifierVersion,
  };
  const deposit = await prove("deposit", depositInput,
    [chainId, contractAddress, verifierVersion, asset, inputAmount, secondNote]);
  await (await token.mint(await signer.getAddress(), inputAmount)).wait();
  await (await token.approve(fixture.pool, inputAmount)).wait();
  await (await pool.deposit(...deposit)).wait();
  const firstPath = tree(poseidon, [firstNote, secondNote], 0);
  const secondPath = tree(poseidon, [firstNote, secondNote], 1);
  if (await pool.currentRoot() !== firstPath.root) throw new Error("fixture root mismatch");
  const template = JSON.parse(readFileSync(resolve(artifactBuild, "transfer-input.json"), "utf8"));
  const expiry = BigInt((await provider.getBlock("latest")).timestamp) + 3600n;
  const recipientSpendTag = poseidon([2006n, 123n]);
  const changeSpendSecret = 456n, changeSpendTag = poseidon([2006n, changeSpendSecret]), changeSalt = 789n;
  const transfer = async (salt, path, nonce) => {
    const fields = Object.fromEntries(["dealId", "revision", "transcriptRootLo", "transcriptRootHi",
      "serviceDigestLo", "serviceDigestHi", "settlementNonceHi", "blinding"]
      .map((name) => [name, BigInt(template[name])]));
    const settlementNonceLo = nonce;
    const paymentSalt = poseidon([2009n, domain, settlementNonceLo, fields.settlementNonceHi]);
    const termsA = poseidon([2001n, domain, fields.dealId, fields.revision, fields.transcriptRootLo, fields.transcriptRootHi]);
    const termsB = poseidon([fields.serviceDigestLo, fields.serviceDigestHi, settlementNonceLo, fields.settlementNonceHi, asset, amount]);
    const termsC = poseidon([...buyer, ...seller, recipientSpendTag, paymentSalt]);
    const dealCommitment = poseidon([2002n, termsA, termsB, termsC, fields.blinding, expiry]);
    const dealNullifier = poseidon([2003n, domain, ...buyer, settlementNonceLo, fields.settlementNonceHi]);
    const inputNullifier = poseidon([2011n, domain, inputSpendSecret, note(salt)]);
    const paymentCommitment = poseidon([2010n, domain, asset, amount, ...seller, recipientSpendTag, paymentSalt]);
    const changeCommitment = poseidon([2010n, domain, asset, changeAmount, ...buyer, changeSpendTag, changeSalt]);
    const buyerSig = eddsa.signPoseidon(buyerPrivate, eddsa.poseidon([2004n, domain, dealCommitment]));
    const sellerSig = eddsa.signPoseidon(sellerPrivate, eddsa.poseidon([2005n, domain, dealCommitment]));
    const input = {
      ...fields, chainId, contractAddress, verifierVersion, asset, amount, dealCommitment, dealNullifier,
      root: path.root, inputNullifier, paymentCommitment, changeCommitment, expiry, settlementNonceLo,
      buyerAx: buyer[0], buyerAy: buyer[1], sellerAx: seller[0], sellerAy: seller[1], recipientSpendTag,
      buyerR8x: F.toObject(buyerSig.R8[0]), buyerR8y: F.toObject(buyerSig.R8[1]), buyerS: buyerSig.S,
      sellerR8x: F.toObject(sellerSig.R8[0]), sellerR8y: F.toObject(sellerSig.R8[1]), sellerS: sellerSig.S,
      inputAmount, inputSalt: salt, inputSpendSecret, pathElements: path.pathElements, pathIndices: path.pathIndices,
      changeAmount, changeSpendTag, changeSpendSecret, changeSalt,
    };
    const proof = await prove("transfer", input, [chainId, contractAddress, verifierVersion, asset,
      dealCommitment, dealNullifier, path.root, inputNullifier, paymentCommitment, changeCommitment, expiry]);
    return pool.interface.encodeFunctionData("transferPrivate", proof);
  };
  console.log(JSON.stringify({ ...fixture, sponsor: (await sponsor.getAddress()).toLowerCase(),
    valid: await transfer(inputSalt, firstPath, 1n),
    spent: await transfer(inputSalt, firstPath, 2n),
    reused: await transfer(secondSalt, secondPath, 3n),
    unknown_root: await transfer(inputSalt, tree(poseidon, [firstNote, secondNote + 1n], 0), 4n),
    test_only: true, live_transactions_sent: 0,
  }));
} finally {
  provider.destroy();
  await globalThis.curve_bn128?.terminate();
}
