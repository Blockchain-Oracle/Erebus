// Regenerates sdk/core/tests/fixtures/m5-note-vector.json for the candidate v2 note hashes.
// Known local fixture inputs; not release evidence.
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { buildPoseidon } from "circomlibjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const out = resolve(root, "../../sdk/core/tests/fixtures/m5-note-vector.json");
const poseidon = await buildPoseidon();
const F = poseidon.F;
const p = (values) => BigInt(F.toObject(poseidon(values)));
const hex = (value, bytes = 32) => BigInt(value).toString(16).padStart(bytes * 2, "0");
const field = (label) => BigInt("0x" + createHash("sha256").update(`EREBUS_M5_TEST_ONLY_${label}`).digest("hex")) % BigInt(F.p);

const chainId = 10143n;
const pool = BigInt("0x5FC8d32690cc91D4c39d9d3abcBD16989F875707");
const verifierVersion = 2n;
const asset = BigInt("0x5FbDB2315678afecb367f032d93F642f64180aa3");
const noteDomain = p([chainId, pool, verifierVersion]);

const owner = (label) => {
  // BabyJubJub owner keys from the old fixture are reused for input/payment.
  return null;
};

const inputOwner = BigInt("0x207da3420047cd4a38bc1f0dd092ea03cbdbd5f01fa45d877bf4b8a532e4c91a1497069db1d64ed9ed6670a822111b4c8bd0c3cd00dccb6af96d890bd61cbfd7");
const inputSecret = BigInt("0x0182c996f69200665dc3f75a04f6f370c62e184e7b106fb66350c993b4edca01");
const inputSalt = BigInt("0x0ca06b7848111af41bea1972745aa9d58cfb89a3d88f16aae61283a7773f4ed8");
const paymentOwner = BigInt("0x2e58b2da0e461747fc0428a4426416fe5817448d1e0339dd327768324f8950582e5cec5d949513def1ca821987dbbab5e1ac1e017486ae7072e51ce285fd4951");
const paymentSecret = BigInt("0x14f496f58584c63e2ff60b82e9c51c1fbf04840113d69ed406e4f734b6c4c6d3");
const paymentSalt = BigInt("0x114f072a6e257377b0b772a522ef3c8a3d0179ea18b65df9c481c0626044be9e");

const split = (value) => {
  const raw = BigInt(value).toString(16).padStart(128, "0");
  return [BigInt("0x" + raw.slice(0, 64)), BigInt("0x" + raw.slice(64))];
};
const spendTag = (secret) => p([2006n, secret]);
const commitment = (amount, ownerValue, tag, salt) => {
  const [ax, ay] = split(ownerValue);
  return p([2010n, noteDomain, asset, amount, ax, ay, tag, salt]);
};
const nullifier = (secret, note) => p([2011n, noteDomain, secret, note]);

const inputTag = spendTag(inputSecret);
const inputCommitment = commitment(150n, inputOwner, inputTag, inputSalt);
const paymentTag = spendTag(paymentSecret);
const paymentCommitment = commitment(70n, paymentOwner, paymentTag, paymentSalt);
const changeSecret = field("change-spend");
const changeSalt = field("change-salt");
const changeTag = spendTag(changeSecret);
const changeCommitment = commitment(80n, inputOwner, changeTag, changeSalt);

const zeros = [0n];
for (let level = 0; level < 20; level += 1) zeros.push(p([zeros[level], zeros[level]]));
const parent = (a, b) => p([a, b]);
let inputRoot = inputCommitment;
for (let level = 0; level < 20; level += 1) inputRoot = parent(inputRoot, zeros[level]);
let nodes = [inputCommitment, paymentCommitment, changeCommitment];
for (let level = 0; level < 20; level += 1) {
  const next = [];
  for (let i = 0; i < nodes.length; i += 2) next.push(parent(nodes[i], nodes[i + 1] ?? zeros[level]));
  nodes = next;
}
const fullRoot = nodes[0];

const vector = {
  chainId: chainId.toString(),
  poolHex: hex(pool, 20),
  verifierVersion: verifierVersion.toString(),
  noteDomainHex: hex(noteDomain),
  assetHex: hex(asset, 20),
  input: {
    amount: "150",
    ownerHex: hex(inputOwner, 64),
    spendSecretHex: hex(inputSecret),
    spendTagHex: hex(inputTag),
    saltHex: hex(inputSalt),
    commitmentHex: hex(inputCommitment),
    nullifierHex: hex(nullifier(inputSecret, inputCommitment)),
    index: 0,
    rootHex: hex(inputRoot),
  },
  payment: {
    amount: "70",
    ownerHex: hex(paymentOwner, 64),
    spendSecretHex: hex(paymentSecret),
    spendTagHex: hex(paymentTag),
    saltHex: hex(paymentSalt),
    commitmentHex: hex(paymentCommitment),
    nullifierHex: hex(nullifier(paymentSecret, paymentCommitment)),
    index: 1,
    rootHex: hex(fullRoot),
  },
  change: {
    amount: "80",
    ownerHex: hex(inputOwner, 64),
    spendSecretHex: hex(changeSecret),
    spendTagHex: hex(changeTag),
    saltHex: hex(changeSalt),
    commitmentHex: hex(changeCommitment),
    nullifierHex: hex(nullifier(changeSecret, changeCommitment)),
  },
  changeCommitmentHex: hex(changeCommitment),
};
writeFileSync(out, `${JSON.stringify(vector, null, 2)}\n`);
console.log("wrote", out);
console.log("noteDomain", vector.noteDomainHex, "inputRoot", vector.input.rootHex, "fullRoot", vector.payment.rootHex);
