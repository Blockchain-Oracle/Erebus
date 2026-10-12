// Read-only handoff verification. Names and hashes cannot prove human independence.
import { readFile, lstat, open } from "node:fs/promises";
import { constants } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as binFileUtils from "@iden3/binfileutils";
import * as zkeyUtils from "../node_modules/snarkjs/src/zkey_utils.js";
import { blake2b } from "@noble/hashes/blake2b";
import * as snarkjs from "snarkjs";
import { hashFile } from "./build-artifact-bundle.mjs";

export async function readBoundedJsonBytes(path, message) {
  let file;
  try {
    file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const metadata = await file.stat();
    if (!metadata.isFile() || metadata.size === 0 || metadata.size > 65536) throw new Error(message);
    // One extra byte detects growth without allowing an unbounded read.
    const bytes = Buffer.alloc(65537);
    let length = 0;
    while (length < bytes.length) {
      const { bytesRead } = await file.read(bytes, length, bytes.length - length, null);
      if (bytesRead === 0) break;
      length += bytesRead;
    }
    if (length === 0 || length > 65536) throw new Error(message);
    return bytes.subarray(0, length);
  } catch {
    throw new Error(message);
  } finally {
    if (file) await file.close();
  }
}

function fields(value, names) {
  if (value === null || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some((key) => !names.includes(key)) || names.some((key) => !(key in value))) throw new Error("invalid handoff fields");
}

export function validatePlan(plan) {
  fields(plan, ["version", "test_only", "contributor", "r1cs", "ptau", "before", "after"]);
  if (plan.version !== 1 || typeof plan.test_only !== "boolean" || typeof plan.contributor !== "string" || !/^[A-Za-z0-9_.@ -]{1,64}$/.test(plan.contributor) || !plan.contributor.trim()) throw new Error("invalid handoff identity");
  for (const key of ["r1cs", "ptau", "before", "after"]) {
    fields(plan[key], ["path", "sha256"]);
    if (typeof plan[key].path !== "string" || !plan[key].path || !/^[0-9a-f]{64}$/.test(plan[key].sha256)) throw new Error("invalid file identity");
  }
  if (plan.before.sha256 === plan.after.sha256) throw new Error("handoff requires a changed key");
  return plan;
}

export async function contributionHistory(path) {
  const { fd, sections } = await binFileUtils.readBinFile(path, "zkey", 2);
  let curve;
  try {
    const header = await zkeyUtils.readHeader(fd, sections, false, { singleThread: true });
    if (header.protocol !== "groth16") throw new Error("handoff requires Groth16");
    curve = header.curve;
    const params = await zkeyUtils.readMPCParams(fd, curve, sections);
    return {
      circuit_hash: Buffer.from(params.csHash).toString("hex"),
      contributions: params.contributions.map((entry) => {
        const hash = blake2b.create({ dkLen: 64 });
        zkeyUtils.hashPubKey(hash, curve, entry);
        return { name: entry.name ?? "", type: entry.type, hash: Buffer.from(hash.digest()).toString("hex") };
      }),
    };
  } finally {
    await fd.close();
    if (curve) await curve.terminate();
  }
}

export function verifyHistory(before, after, contributor) {
  if (before.circuit_hash !== after.circuit_hash) throw new Error("different circuit history");
  if (after.contributions.length !== before.contributions.length + 1) throw new Error("handoff must append exactly one contribution");
  for (let index = 0; index < before.contributions.length; index++) {
    if (JSON.stringify(before.contributions[index]) !== JSON.stringify(after.contributions[index])) throw new Error("previous contribution history changed");
  }
  const appended = after.contributions.at(-1);
  if (appended.type !== 0 || appended.name !== contributor) throw new Error("expected named entropy contribution, not a beacon or different name");
  return appended;
}

export async function verifyStep(rawPlan) {
  const plan = validatePlan(rawPlan);
  const packageVersion = JSON.parse(await readFile(fileURLToPath(new URL("../node_modules/snarkjs/package.json", import.meta.url)), "utf8")).version;
  if (packageVersion !== "0.7.6") throw new Error("requires pinned snarkjs 0.7.6 metadata reader");
  for (const key of ["r1cs", "ptau", "before", "after"]) {
    const metadata = await lstat(plan[key].path);
    if (!metadata.isFile() || metadata.size === 0 || metadata.size > 2 * 1024 ** 3 || await hashFile(plan[key].path) !== plan[key].sha256) throw new Error(`file identity mismatch: ${key}`);
  }
  const before = await contributionHistory(plan.before.path);
  const after = await contributionHistory(plan.after.path);
  const contribution = verifyHistory(before, after, plan.contributor);
  for (const key of ["before", "after"]) {
    if (!await snarkjs.zKey.verifyFromR1cs(plan.r1cs.path, plan.ptau.path, plan[key].path)) throw new Error(`setup verification failed: ${key}`);
  }
  for (const key of ["r1cs", "ptau", "before", "after"]) {
    if (await hashFile(plan[key].path) !== plan[key].sha256) throw new Error(`input changed during verification: ${key}`);
  }
  return {
    version: 1, consistent: true, test_only_declared: plan.test_only,
    r1cs_sha256: plan.r1cs.sha256, ptau_sha256: plan.ptau.sha256,
    before_sha256: plan.before.sha256, after_sha256: plan.after.sha256,
    contribution, contribution_count: after.contributions.length,
    trust_verified: false,
    note: "Read-only consistency evidence. Does not verify phase-1 provenance, participant identity, independence, secret destruction, or an audit.",
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error("usage: node verify-phase2-step.mjs <hash-pinned-plan.json>");
    const bytes = await readBoundedJsonBytes(process.argv[2], "invalid step plan file");
    let plan;
    try { plan = JSON.parse(bytes.toString("utf8")); }
    catch { throw new Error("invalid step plan JSON"); }
    console.log(JSON.stringify(await verifyStep(plan)));
    process.exit(0);
  } catch (error) { console.error(error.message); process.exit(1); }
}
