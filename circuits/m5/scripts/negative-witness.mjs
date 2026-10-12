// Local constraint tests only: no setup, proof generation, network, or witness files.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

const build = fileURLToPath(new URL("../build/", import.meta.url));
const manifest = JSON.parse(readFileSync(resolve(build, "artifact-manifest.json")));
const summary = { scope: "local_candidate_constraints_not_audit", circuits: {} };

const bounds = {
  chainId: 64, contractAddress: 160, verifierVersion: 32, asset: 160,
  amount: 128, recipient: 160, dealId: 128, revision: 32, expiry: 64,
  transcriptRootLo: 128, transcriptRootHi: 128,
  serviceDigestLo: 128, serviceDigestHi: 128,
  settlementNonceLo: 128, settlementNonceHi: 128,
  inputAmount: 128, changeAmount: 128,
};
const requiredNonzero = {
  deposit: ["amount", "spendTag"],
  transfer: ["amount", "asset", "revision", "blinding", "inputSpendSecret",
    "recipientSpendTag", "changeSpendSecret", "buyerS", "sellerS"],
  withdraw: ["amount", "spendSecret"],
};

for (const name of ["deposit", "transfer", "withdraw"]) {
  const wasm = readFileSync(resolve(build, `${name}_js/${name}.wasm`));
  assert.equal(createHash("sha256").update(wasm).digest("hex"),
    manifest.circuits[name].wasmSha256, `${name}: artifact hash mismatch`);
  // Circom emits CommonJS .js inside this ESM package. Load it without rewriting artifacts.
  const module = { exports: {} };
  runInNewContext(readFileSync(resolve(build, `${name}_js/witness_calculator.js`), "utf8"), {
    module, WebAssembly,
    console: { log() { throw new Error("unexpected witness diagnostic"); } },
  });
  const builder = module.exports;
  const calculator = await builder(wasm);
  const input = JSON.parse(readFileSync(resolve(build, `${name}-input.json`)));
  let rejected = 0;
  await calculator.calculateWitness(input, true);

  async function reject(label, mutate) {
    const changed = structuredClone(input);
    mutate(changed);
    // A loading/schema error is not evidence that a circuit constraint rejected a witness.
    await assert.rejects(calculator.calculateWitness(changed, true), /Assert Failed/,
      `${name}: ${label} must fail a constraint`);
    rejected++;
  }

  // Each scalar is bound to the unmodified agreement, opening, or membership root.
  for (const [field, value] of Object.entries(input)) {
    if (typeof value === "string") {
      await reject(`changed ${field}`, changed => { changed[field] = String(BigInt(value) + 1n); });
    }
  }
  for (const field of requiredNonzero[name]) {
    await reject(`zero ${field}`, changed => { changed[field] = "0"; });
  }
  for (const [field, bits] of Object.entries(bounds)) {
    if (field in input) {
      await reject(`overflow ${field}`, changed => { changed[field] = String(1n << BigInt(bits)); });
    }
  }
  if (input.pathElements) {
    for (let index = 0; index < input.pathElements.length; index++) {
      await reject(`sibling ${index}`, changed => {
        changed.pathElements[index] = String(BigInt(changed.pathElements[index]) + 1n);
      });
      await reject(`direction ${index}`, changed => {
        changed.pathIndices[index] = String(1n - BigInt(changed.pathIndices[index]));
      });
      await reject(`nonbinary direction ${index}`, changed => { changed.pathIndices[index] = "2"; });
    }
  }
  if (name === "transfer") {
    const subOrder = "2736030358979909402780800718157159386076813972158567259200215660948447373041";
    for (const role of ["buyer", "seller"]) {
      await reject(`${role} scalar at subgroup order`, changed => { changed[`${role}S`] = subOrder; });
      await reject(`${role} identity key`, changed => {
        changed[`${role}Ax`] = "0";
        changed[`${role}Ay`] = "1";
      });
    }
    await reject("conserved but unauthorized price", changed => {
      changed.amount = String(BigInt(input.amount) + 1n);
      changed.changeAmount = String(BigInt(input.changeAmount) - 1n);
    });
  }
  // Reuse after failed witnesses must not poison the calculator's next valid request.
  await calculator.calculateWitness(input, true);
  summary.circuits[name] = { valid_controls: 2, rejected_mutations: rejected };
}
console.log(JSON.stringify(summary));
