// Cryptographic transcript consistency is separate from its provenance review.
import { readFile, lstat } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import * as snarkjs from "snarkjs";
import { hashFile } from "./build-artifact-bundle.mjs";

export function validatePlan(plan) {
  const required = ["version", "test_only", "ptau", "provenance"];
  if (plan === null || typeof plan !== "object" || Array.isArray(plan) || required.some((key) => !(key in plan)) || Object.keys(plan).some((key) => ![...required, "published_pin"].includes(key)) || plan.version !== 1 || typeof plan.test_only !== "boolean") throw new Error("invalid phase-1 plan");
  for (const key of ["ptau", "provenance"]) {
    const file = plan[key];
    if (file === null || typeof file !== "object" || Array.isArray(file) || Object.keys(file).sort().join(",") !== "path,sha256" || typeof file.path !== "string" || !file.path || !/^[0-9a-f]{64}$/.test(file.sha256)) throw new Error("invalid phase-1 file identity");
  }
  if ("published_pin" in plan) {
    const pin = plan.published_pin;
    if (pin === null || typeof pin !== "object" || Array.isArray(pin) || Object.keys(pin).sort().join(",") !== "algorithm,digest" || !["sha256", "blake2b512"].includes(pin.algorithm) || typeof pin.digest !== "string" || !new RegExp(`^[0-9a-f]{${pin.algorithm === "sha256" ? 64 : 128}}$`).test(pin.digest)) throw new Error("invalid published transcript pin");
  }
  return plan;
}

export async function publishedDigest(path, algorithm) {
  if (!["sha256", "blake2b512"].includes(algorithm)) throw new Error("unsupported published digest");
  const hash = createHash(algorithm);
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

export async function verifyPhase1(rawPlan) {
  const plan = validatePlan(rawPlan);
  for (const key of ["ptau", "provenance"]) {
    const metadata = await lstat(plan[key].path);
    const maxBytes = key === "ptau" ? 2 * 1024 ** 3 : 1024 * 1024;
    if (!metadata.isFile() || metadata.size === 0 || metadata.size > maxBytes || await hashFile(plan[key].path) !== plan[key].sha256) throw new Error(`phase-1 identity mismatch: ${key}`);
  }
  if (plan.published_pin && await publishedDigest(plan.ptau.path, plan.published_pin.algorithm) !== plan.published_pin.digest) throw new Error("published transcript digest mismatch");
  if (!await snarkjs.powersOfTau.verify(plan.ptau.path)) throw new Error("phase-1 cryptographic verification failed");
  for (const key of ["ptau", "provenance"]) {
    if (await hashFile(plan[key].path) !== plan[key].sha256) throw new Error(`phase-1 input changed: ${key}`);
  }
  return {
    version: 1, consistent: true, test_only_declared: plan.test_only,
    ptau_sha256: plan.ptau.sha256, provenance_record_sha256: plan.provenance.sha256,
    published_pin: plan.published_pin ?? null,
    provenance_reviewed: false, trust_verified: false,
    note: "The provenance record is hash-pinned, not endorsed. Human review of origin, participants and destruction assumptions remains required.",
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error("usage: node verify-phase1.mjs <hash-pinned-plan.json>");
    console.log(JSON.stringify(await verifyPhase1(JSON.parse(await readFile(process.argv[2], "utf8")))));
    process.exit(0);
  } catch (error) { console.error(error.message); process.exit(1); }
}
