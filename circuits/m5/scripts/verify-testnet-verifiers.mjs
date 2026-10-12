// Local, in-process EVM verification of test-only generated verifiers. No RPC or deployment.
import { readFile, writeFile, mkdtemp, mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { execFileSync } from "node:child_process";
import * as snarkjs from "snarkjs";
import solc from "solc";
import { keccak256 } from "ethers";
import { hashFile, artifactIdentity } from "./build-artifact-bundle.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const kinds = ["deposit", "transfer", "withdraw"];
const scalar = (value) => {
  if (!/^0x[0-9a-f]+$/i.test(value) && !/^[0-9]+$/.test(value)) throw new Error("invalid proof scalar");
  return `uint256(${value})`;
};
const array = (values) => `[${values.map(scalar).join(",")}]`;

export async function verifyTestnetVerifiers(bundle, manifestHash) {
  const manifestPath = resolve(bundle, "manifest.json");
  if (!/^[0-9a-f]{64}$/.test(manifestHash) || await hashFile(manifestPath) !== manifestHash) throw new Error("manifest pin mismatch");
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (manifest.version !== 2 || manifest.test_only !== true || manifest.circuits.length !== 3) throw new Error("requires test-only v2 bundle");
  const directory = await mkdtemp("/private/tmp/erebus-verifier-evm-");
  await mkdir(resolve(directory, "test"));
  const imports = [];
  const tests = [];
  for (const kind of kinds) {
    const records = manifest.circuits.filter((circuit) => circuit.kind === kind);
    if (records.length !== 1) throw new Error("invalid circuit list");
    const record = records[0];
    const source = await readFile(resolve(bundle, `${kind}-verifier.sol`), "utf8");
    const compiled = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity",
      sources: { "verifier.sol": { content: source } }, settings: { optimizer: { enabled: false, runs: 200 },
        evmVersion: "paris", outputSelection: { "*": { "*": ["evm.deployedBytecode.object"] } } } })));
    const runtime = compiled.contracts?.["verifier.sol"]?.Groth16Verifier?.evm.deployedBytecode.object;
    if (!solc.version().startsWith("0.8.24+") || !runtime
        || keccak256(`0x${runtime}`).slice(2) !== record.identity.verifier_runtime_keccak256) throw new Error("verifier runtime pin mismatch");
    for (const [field, path] of [["wasm", `${kind}_js/${kind}.wasm`], ["r1cs", `${kind}.r1cs`], ["zkey", `${kind}.zkey`]]) {
      const identity = await artifactIdentity(resolve(bundle, path));
      if (identity.sha256 !== record[field].sha256 || identity.bytes !== record[field].bytes) throw new Error("artifact pin mismatch");
    }
    const input = JSON.parse(await readFile(resolve(root, `build/${kind}-input.json`), "utf8"));
    const { proof, publicSignals } = await snarkjs.groth16.fullProve(input, resolve(bundle, `${kind}_js/${kind}.wasm`), resolve(bundle, `${kind}.zkey`));
    const calldata = JSON.parse(`[${await snarkjs.groth16.exportSolidityCallData(proof, publicSignals)}]`);
    if (publicSignals.length !== { deposit: 6, transfer: 11, withdraw: 8 }[kind]) throw new Error("invalid input count");
    await writeFile(resolve(directory, `${kind}-verifier.sol`), source, { flag: "wx" });
    imports.push(`import {Groth16Verifier as ${kind}Verifier} from "../${kind}-verifier.sol";`);
    tests.push(`function test_${kind}_valid_and_mutated() public {
      ${kind}Verifier verifier = new ${kind}Verifier();
      uint256[2] memory a = ${array(calldata[0])};
      uint256[2][2] memory b = [${calldata[1].map(array).join(",")}];
      uint256[2] memory c = ${array(calldata[2])};
      uint256[${publicSignals.length}] memory input = ${array(calldata[3])};
      require(verifier.verifyProof(a,b,c,input), "valid proof rejected");
      input[0] += 1;
      require(!verifier.verifyProof(a,b,c,input), "mutated input accepted");
    }`);
  }
  await writeFile(resolve(directory, "foundry.toml"), '[profile.default]\nsrc = "."\ntest = "test"\nsolc_version = "0.8.24"\nevm_version = "paris"\noptimizer = false\n', { flag: "wx" });
  const harness = resolve(directory, "test/GeneratedVerifier.t.sol");
  await writeFile(harness, `pragma solidity 0.8.24;\n${imports.join("\n")}\ncontract GeneratedVerifierTest {\n${tests.join("\n")}\n}\n`, { flag: "wx", mode: 0o600 });
  const result = execFileSync("forge", ["test", "--offline", "--root", directory, "--match-contract", "GeneratedVerifierTest"], { encoding: "utf8", timeout: 180000 });
  if (!/3 passed; 0 failed/.test(result)) throw new Error("unexpected EVM test result");
  if (await hashFile(manifestPath) !== manifestHash) throw new Error("manifest changed");
  const receipt = { test_only: true, independent: false, live_chain: false, evm_tests_passed: 3,
    manifest_sha256: manifestHash, harness_sha256: await hashFile(harness), directory };
  await writeFile(resolve(directory, "receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx", mode: 0o600 });
  return receipt;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 4) throw new Error("invalid arguments");
    console.log(JSON.stringify(await verifyTestnetVerifiers(resolve(process.argv[2]), process.argv[3])));
    process.exit(0);
  } catch { console.error("test-only EVM verifier qualification failed"); process.exit(1); }
}
