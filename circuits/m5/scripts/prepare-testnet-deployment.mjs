// Offline, test-only deployment planning. No key loading, RPC, signing or broadcasting.
import { readFile, writeFile, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { ContractFactory, getAddress, getCreateAddress, keccak256, toBeHex, zeroPadValue } from "ethers";
import { poseidonContract } from "circomlibjs";
import solc from "solc";
import { stageTestBundle } from "./stage-test-bundle.mjs";
import { hashFile, sourceInventory } from "./build-artifact-bundle.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const repo = resolve(root, "../..");
const witnessPaths = ["sdk/core/src/shielded.rs", "sdk/core/src/shielded_auth.rs",
  "sdk/shielded/src/lib.rs", "circuits/m5/scripts/pool.mjs", "circuits/m5/scripts/native-fixture.mjs"];
const digest = (value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const address = (value) => {
  try {
    const result = getAddress(value);
    if (BigInt(result) === 0n) throw Error();
    return result;
  } catch { throw new Error("invalid nonzero address"); }
};

export function validateDeploymentPlan(plan) {
  const fields = ["version", "test_only", "chain_id", "bundle_dir", "manifest_sha256",
    "deployer", "starting_nonce", "assets"];
  if (!plan || Object.keys(plan).sort().join() !== fields.sort().join()
      || plan.version !== 1 || plan.test_only !== true || plan.chain_id !== 10143) {
    throw new Error("requires a prepare-only Monad testnet plan");
  }
  address(plan.deployer);
  if (!Number.isSafeInteger(plan.starting_nonce) || plan.starting_nonce < 0
      || plan.starting_nonce > Number.MAX_SAFE_INTEGER - 6) throw new Error("invalid starting nonce");
  if (typeof plan.bundle_dir !== "string" || !plan.bundle_dir || !digest(plan.manifest_sha256)) {
    throw new Error("bundle directory and explicit manifest pin required");
  }
  if (!Array.isArray(plan.assets) || plan.assets.length < 1 || plan.assets.length > 2) {
    throw new Error("requires one or two asset-specific pools");
  }
  const names = new Set();
  const addresses = new Set();
  for (const asset of plan.assets) {
    if (!asset || Object.keys(asset).sort().join() !== "address,name,runtime_keccak256"
        || typeof asset.name !== "string" || !/^[a-z][a-z0-9_-]{0,31}$/.test(asset.name) || !digest(asset.runtime_keccak256)
        || /^0+$/.test(asset.runtime_keccak256)) throw new Error("invalid asset identity");
    const normalized = address(asset.address);
    if (names.has(asset.name) || addresses.has(normalized)) throw new Error("duplicate asset");
    names.add(asset.name);
    addresses.add(normalized);
  }
  return plan;
}

// circomlibjs 0.1.7's two-input Poseidon uses this fixed CODECOPY/RETURN loader.
export function poseidonRuntime(initCode) {
  const match = /^0x38600c60003961([0-9a-f]{4})6000f3([0-9a-f]+)$/.exec(initCode);
  if (!match || match[2].length / 2 !== Number.parseInt(match[1], 16)) {
    throw new Error("unsupported Poseidon constructor loader");
  }
  return `0x${match[2]}`;
}

function immutableRuntime(artifact, ast, values) {
  const names = new Map();
  function visit(node) {
    if (!node || typeof node !== "object") return;
    if (node.nodeType === "VariableDeclaration" && node.mutability === "immutable") {
      names.set(String(node.id), node.name);
    }
    for (const child of Object.values(node)) {
      if (Array.isArray(child)) child.forEach(visit);
      else if (child && typeof child === "object") visit(child);
    }
  }
  visit(ast);
  const runtime = Buffer.from(artifact.evm.deployedBytecode.object, "hex");
  for (const [id, references] of Object.entries(artifact.evm.deployedBytecode.immutableReferences)) {
    const value = values[names.get(id)];
    if (value === undefined) throw new Error("unbound pool immutable");
    const word = Buffer.from(zeroPadValue(toBeHex(BigInt(value)), 32).slice(2), "hex");
    for (const { start, length } of references) {
      if (length !== 32 || start + length > runtime.length) throw new Error("invalid immutable range");
      word.copy(runtime, start);
    }
  }
  return `0x${runtime.toString("hex")}`;
}

export async function prepareTestnetDeployment(rawPlan) {
  const plan = validateDeploymentPlan(rawPlan);
  const bundle = resolve(plan.bundle_dir);
  if (await hashFile(resolve(bundle, "manifest.json")) !== plan.manifest_sha256) {
    throw new Error("manifest pin mismatch");
  }
  const manifest = JSON.parse(await readFile(resolve(bundle, "manifest.json"), "utf8"));
  const inventory = JSON.parse(await readFile(resolve(bundle, "source-inventory.json"), "utf8"));
  if (manifest.chain_id !== 10143 || manifest.verifier_version !== 2
      || JSON.stringify((await sourceInventory()).files) !== JSON.stringify(inventory.files)) {
    throw new Error("candidate circuit/domain mismatch");
  }
  if (!Array.isArray(inventory.witness_sources)
      || inventory.witness_sources.map((r) => r.path).sort().join() !== [...witnessPaths].sort().join()) {
    throw new Error("invalid witness source inventory");
  }
  for (const record of inventory.witness_sources) {
    if (await hashFile(resolve(repo, record.path)) !== record.sha256) throw new Error("stale witness source inventory");
  }
  const staged = await stageTestBundle(bundle, plan.manifest_sha256);
  try {
    if (!solc.version().startsWith("0.8.24+")) throw new Error("requires solc 0.8.24");
    const poolSource = await readFile(resolve(repo, "contracts/evm/src/ErebusShieldedPool.sol"), "utf8");
    const compiled = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity",
      sources: { "ErebusShieldedPool.sol": { content: poolSource } },
      settings: { optimizer: { enabled: true, runs: 200 }, evmVersion: "paris",
        outputSelection: { "*": { "": ["ast"], "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode"] } } } })));
    if (compiled.errors?.some((e) => e.severity === "error")) throw new Error("pool compilation failed");
    const pool = compiled.contracts["ErebusShieldedPool.sol"].ErebusShieldedPool;
    const calls = [];
    const deployer = address(plan.deployer);
    async function add(name, abi, initCode, args, runtime) {
      const nonce = plan.starting_nonce + calls.length;
      const predicted = getCreateAddress({ from: deployer, nonce });
      const request = await new ContractFactory(abi, initCode).getDeployTransaction(...args);
      calls.push({ name, from: deployer, chain_id: 10143, nonce, value: "0",
        predicted_address: predicted, data: request.data, initcode_keccak256: keccak256(request.data),
        runtime_keccak256: keccak256(runtime) });
      return predicted;
    }
    const poseidonCode = poseidonContract.createCode(2);
    const poseidon = await add("poseidon", poseidonContract.generateABI(2), poseidonCode, [], poseidonRuntime(poseidonCode));
    const verifiers = {};
    for (const kind of ["deposit", "transfer", "withdraw"]) {
      const artifact = staged.verifiers[kind];
      verifiers[kind] = await add(kind, artifact.abi, `0x${artifact.evm.bytecode.object}`, [],
        `0x${artifact.evm.deployedBytecode.object}`);
    }
    const pools = [];
    for (const asset of plan.assets) {
      const values = { asset: address(asset.address), poseidonTwo: poseidon,
        depositVerifier: verifiers.deposit, transferVerifier: verifiers.transfer,
        withdrawVerifier: verifiers.withdraw, verifierVersion: 2 };
      const runtime = immutableRuntime(pool, compiled.sources["ErebusShieldedPool.sol"].ast, values);
      const predicted = await add(`pool_${asset.name}`, pool.abi, `0x${pool.evm.bytecode.object}`,
        Object.values(values), runtime);
      pools.push({ name: asset.name, pool: predicted, asset: values.asset,
        asset_runtime_keccak256: asset.runtime_keccak256, verifier_version: 2,
        namespace: "eip155:10143", runtime_pins: { pool: keccak256(runtime).slice(2),
          poseidon: calls[0].runtime_keccak256.slice(2),
          ...Object.fromEntries(calls.slice(1, 4).map((c) => [c.name, c.runtime_keccak256.slice(2)])) },
        artifact_domain_requires_rebinding: true, deployment_block: null, deployment_hash: null });
    }
    if (await hashFile(resolve(bundle, "manifest.json")) !== plan.manifest_sha256
        || await readFile(resolve(repo, "contracts/evm/src/ErebusShieldedPool.sol"), "utf8") !== poolSource) {
      throw new Error("candidate changed during preparation");
    }
    return { version: 1, mode: "prepare_only", test_only: true, independent: false, audited: false,
      authorized_for_broadcast: false, live_transactions_sent: 0, manifest_sha256: plan.manifest_sha256,
      pool_source_sha256: await hashFile(resolve(repo, "contracts/evm/src/ErebusShieldedPool.sol")),
      compiler: { solc: solc.version(), optimizer: { enabled: true, runs: 200 }, evm_version: "paris" },
      calls, pools, required_before_broadcast: ["owner approval and budgets", "current pending nonce",
        "two finalized RPC asset/runtime checks", "gas estimation and funding", "durable signed broadcast journal"],
      required_after_deployment: ["finalized creation receipts and runtime checks",
        "constructor getter checks", "deployment-bound artifact manifests and hosted URLs"] };
  } finally { await rm(staged.build, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 4) throw new Error("usage: prepare-testnet-deployment.mjs <plan.json> <output.json>");
    const result = await prepareTestnetDeployment(JSON.parse(await readFile(process.argv[2], "utf8")));
    await writeFile(process.argv[3], `${JSON.stringify(result, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    console.log(JSON.stringify({ status: "prepared", calls: result.calls.length,
      pools: result.pools.map((p) => ({ name: p.name, address: p.pool })), live_transactions_sent: 0 }));
  } catch { console.error("testnet deployment preparation failed; nothing broadcast"); process.exitCode = 1; }
}
