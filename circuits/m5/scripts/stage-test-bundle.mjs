// Explicit test-only staging. Does not generate setup, start RPC, or send transactions.
import { createHash } from "node:crypto";
import { readFile, writeFile, mkdtemp, lstat } from "node:fs/promises";
import { resolve } from "node:path";
import { tmpdir } from "node:os";
import solc from "solc";
import { keccak256 } from "ethers";
import { artifactIdentity, copyPinnedArtifact, hashFile } from "./build-artifact-bundle.mjs";

export function compilePinnedVerifier(source, runtimeHash) {
  const output = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity",
    sources: { "verifier.sol": { content: source } }, settings: { optimizer: { enabled: false, runs: 200 },
      evmVersion: "paris", outputSelection: { "*": { "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"] } } } })));
  const artifact = output.contracts?.["verifier.sol"]?.Groth16Verifier;
  const runtime = artifact?.evm.deployedBytecode.object;
  if (!solc.version().startsWith("0.8.24+") || output.errors?.some((error) => error.severity === "error")
      || !runtime || keccak256(`0x${runtime}`).slice(2) !== runtimeHash) throw new Error("verifier runtime pin mismatch");
  return artifact;
}

export async function stageTestBundle(bundle, manifestHash) {
  if (typeof bundle !== "string" || !bundle || !/^[0-9a-f]{64}$/.test(manifestHash)) throw new Error("test bundle requires explicit directory and manifest pin");
  const path = resolve(bundle, "manifest.json");
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > 16384) throw new Error("invalid manifest file");
  const bytes = await readFile(path);
  if (createHash("sha256").update(bytes).digest("hex") !== manifestHash) throw new Error("manifest pin mismatch");
  let manifest;
  try { manifest = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("invalid manifest JSON"); }
  const kinds = ["deposit", "transfer", "withdraw"];
  if (manifest.version !== 2 || manifest.test_only !== true || !Array.isArray(manifest.circuits)
      || manifest.circuits.length !== 3 || kinds.some((kind) => manifest.circuits.filter((c) => c.kind === kind).length !== 1)) throw new Error("requires three test-only v2 circuits");
  const staged = await mkdtemp(resolve(tmpdir(), "erebus-staged-test-bundle-"));
  const artifacts = { version: "m5-single-operator-test-only", test_only: true, manifest_sha256: manifestHash, circuits: {} };
  const verifiers = {};
  for (const kind of kinds) {
    const record = manifest.circuits.find((c) => c.kind === kind);
    if (!record.identity || record.identity.witness_schema_version !== 2) throw new Error("invalid witness identity");
    const files = [["wasm", `${kind}_js/${kind}.wasm`], ["r1cs", `${kind}.r1cs`], ["zkey", `${kind}.zkey`]];
    for (const [field, name] of files) {
      const actual = await artifactIdentity(resolve(bundle, name));
      if (actual.sha256 !== record[field]?.sha256 || actual.bytes !== record[field]?.bytes) throw new Error("artifact pin mismatch");
      await copyPinnedArtifact(resolve(bundle, name), resolve(staged, name), actual);
    }
    for (const [name, hash] of [[`${kind}-verification-key.json`, record.identity.verification_key_sha256],
      ["source-inventory.json", record.identity.source_inventory_sha256]]) {
      if (name === "source-inventory.json" && kind !== "deposit") {
        if (await hashFile(resolve(staged, name)) !== hash) throw new Error("inconsistent source inventory");
        continue;
      }
      const actual = await artifactIdentity(resolve(bundle, name));
      if (actual.sha256 !== hash) throw new Error("identity file mismatch");
      await copyPinnedArtifact(resolve(bundle, name), resolve(staged, name), actual);
    }
    const verifierPath = resolve(bundle, `${kind}-verifier.sol`);
    await artifactIdentity(verifierPath);
    const source = await readFile(verifierPath, "utf8");
    verifiers[kind] = compilePinnedVerifier(source, record.identity.verifier_runtime_keccak256);
    await writeFile(resolve(staged, `${kind}-verifier.sol`), source, { flag: "wx" });
    artifacts.circuits[kind] = { r1csSha256: record.r1cs.sha256, wasmSha256: record.wasm.sha256,
      zkeySha256: record.zkey.sha256, verificationKeySha256: record.identity.verification_key_sha256,
      verifierSourceSha256: createHash("sha256").update(source).digest("hex"),
      verifierRuntimeKeccak256: record.identity.verifier_runtime_keccak256 };
  }
  if (await hashFile(path) !== manifestHash) throw new Error("manifest changed during staging");
  await writeFile(resolve(staged, "artifact-manifest.json"), `${JSON.stringify(artifacts, null, 2)}\n`, { flag: "wx" });
  return { build: staged, artifacts, verifiers };
}
