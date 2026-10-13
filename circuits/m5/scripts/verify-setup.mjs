// Verify the consistency of one M5 circuit setup without claiming trust.
//
// This tool checks that each zkey verifies against its r1cs and the supplied
// powers-of-tau, and prints file identities. Passing does not authenticate the
// ceremony, the contributors, or the entropy behind the keys, and the local
// known-entropy fixtures are not release-safe.
import { createHash } from "node:crypto";
import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const build = process.argv[2] ?? fileURLToPath(new URL("../build/", import.meta.url));
const ptau = process.argv[3] ?? resolve(build, "pot16_final.ptau");
const circuits = ["deposit", "transfer", "withdraw"];
const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const canonical = (value) => {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
};

if (!existsSync(ptau)) throw new Error(`missing ptau: ${ptau}`);
const snarkjs = await import(resolve(fileURLToPath(new URL("../node_modules/snarkjs", import.meta.url)), "build/main.cjs"));

let failed = false;
for (const name of circuits) {
  const r1cs = resolve(build, `${name}.r1cs`);
  const zkey = resolve(build, `${name}.zkey`);
  const vkey = resolve(build, `${name}-verification-key.json`);
  for (const path of [r1cs, zkey, vkey]) {
    if (!existsSync(path)) throw new Error(`missing ${path}`);
  }
  const verified = await snarkjs.zKey.verifyFromR1cs(r1cs, ptau, zkey);
  const exported = await snarkjs.zKey.exportVerificationKey(zkey);
  const verificationKeyMatches = JSON.stringify(canonical(exported)) ===
    JSON.stringify(canonical(JSON.parse(readFileSync(vkey, "utf8"))));
  if (!verified || !verificationKeyMatches) failed = true;
  console.log(JSON.stringify({
    circuit: name,
    r1csSha256: sha256(r1cs),
    zkeySha256: sha256(zkey),
    verificationKeySha256: sha256(vkey),
    zkeyVerifies: verified,
    verificationKeyMatches,
  }));
}
console.log(JSON.stringify({
  ptauSha256: sha256(ptau),
  note: "Consistency check only. Not ceremony provenance or a trust decision; local fixtures are not release-safe.",
}));
process.exit(failed ? 1 : 0);
