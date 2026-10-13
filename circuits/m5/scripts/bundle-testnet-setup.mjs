// Test-only distribution from an executed, single-operator setup. No setup entropy is generated.
import { createHash } from "node:crypto";
import { readFile, mkdir, mkdtemp, writeFile, lstat } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { buildBundle, hashFile, artifactIdentity, copyPinnedArtifact } from "./build-artifact-bundle.mjs";
import { verifyChain } from "./verify-phase2-chain.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const kinds = ["deposit", "transfer", "withdraw"];

async function readPinned(record) {
  if (!record || typeof record.path !== "string" || !/^[0-9a-f]{64}$/.test(record.sha256)) throw new Error("invalid record pin");
  const metadata = await lstat(record.path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > 1048576) throw new Error("invalid record file");
  const bytes = await readFile(record.path);
  if (createHash("sha256").update(bytes).digest("hex") !== record.sha256) throw new Error("record pin mismatch");
  try { return JSON.parse(bytes.toString("utf8")); } catch { throw new Error("invalid record JSON"); }
}

export async function bundleTestnetSetup(plan) {
  const fields = ["test_only", "receipt", "base_url", "chain_id", "settlement_contract", "verifier_version"];
  if (!plan || Object.keys(plan).sort().join(",") !== fields.sort().join(",") || plan.test_only !== true) throw new Error("requires a test-only bundle plan");
  const receipt = await readPinned(plan.receipt);
  if (receipt.test_only !== true || receipt.consistent !== true || receipt.operator_count !== 1
      || receipt.independent !== false || receipt.trust_verified !== false || receipt.destruction_verified !== false
      || !Array.isArray(receipt.circuits) || receipt.circuits.length !== 3) throw new Error("invalid single-operator receipt");
  for (const kind of kinds) if (receipt.circuits.filter((c) => c.kind === kind).length !== 1) throw new Error("requires three circuit keys");
  const inputs = await readPinned(receipt.inputs);
  const chain = await readPinned(receipt.chain_plan);
  if (inputs.test_only !== true || chain.test_only !== true) throw new Error("setup is not test-only");
  const verified = await verifyChain(chain);
  if (!verified.consistent || verified.ptau_sha256 !== inputs.ptau.sha256) throw new Error("setup chain mismatch");
  const candidate = await readPinned(inputs.manifest);
  // This directory is separate from both the candidate fixtures and the executed setup.
  const directory = await mkdtemp("/private/tmp/erebus-testnet-bundle-");
  const staged = resolve(directory, "staged");
  await mkdir(staged, { mode: 0o700 });
  const copied = [];
  async function copy(record, name) {
    const identity = await artifactIdentity(record.path);
    if (identity.sha256 !== record.sha256) throw new Error("setup artifact mismatch");
    const destination = resolve(staged, name);
    await copyPinnedArtifact(record.path, destination, identity);
    copied.push({ path: destination, ...identity });
  }
  await copy(inputs.ptau, "pot16_final.ptau");
  for (const kind of kinds) {
    const record = receipt.circuits.find((c) => c.kind === kind);
    const checked = verified.circuits.find((c) => c.kind === kind);
    const input = inputs.circuits.find((c) => c.kind === kind);
    if (!input || record.zkey.sha256 !== checked.final_zkey_sha256 || input.r1cs.sha256 !== checked.r1cs_sha256
        || candidate.circuits?.[kind]?.r1csSha256 !== input.r1cs.sha256) throw new Error("circuit setup mismatch");
    const wasm = { path: resolve(dirname(inputs.manifest.path), `${kind}_js/${kind}.wasm`), sha256: candidate.circuits[kind].wasmSha256 };
    await copy(wasm, `${kind}_js/${kind}.wasm`);
    await copy(input.r1cs, `${kind}.r1cs`);
    await copy(record.zkey, `${kind}.zkey`);
    await copy(record.vkey, `${kind}-verification-key.json`);
  }
  const result = await buildBundle({ test_only: true, build_dir: staged, output_dir: resolve(directory, "bundle"),
    base_url: plan.base_url, chain_id: plan.chain_id, settlement_contract: plan.settlement_contract, verifier_version: plan.verifier_version });
  // Recheck public inputs after compilation; failures do not issue a completion receipt.
  await readPinned(plan.receipt);
  for (const record of [receipt.inputs, receipt.chain_plan, inputs.manifest]) await readPinned(record);
  for (const record of copied) if (await hashFile(record.path) !== record.sha256) throw new Error("staged artifact changed");
  const completed = { version: 1, test_only: true, independent: false, trust_verified: false, directory,
    setup_receipt: plan.receipt, ...result };
  await writeFile(resolve(directory, "receipt.json"), `${JSON.stringify(completed, null, 2)}\n`, { flag: "wx", mode: 0o600 });
  return completed;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error("invalid arguments");
    const plan = await readPinned({ path: process.argv[2], sha256: await hashFile(process.argv[2]) });
    console.log(JSON.stringify(await bundleTestnetSetup(plan)));
    process.exit(0);
  } catch { console.error("test-only bundle failed; no completion receipt issued"); process.exit(1); }
}
