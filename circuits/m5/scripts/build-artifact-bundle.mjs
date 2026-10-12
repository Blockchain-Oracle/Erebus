// Test-only qualification. This never generates setup entropy or release keys.
import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { copyFile, lstat, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { execFileSync } from "node:child_process";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import solc from "solc";
import * as snarkjs from "snarkjs";
import { keccak256 } from "ethers";

const root = fileURLToPath(new URL("../", import.meta.url));
const kinds = ["deposit", "transfer", "withdraw"];
export async function hashFile(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

export async function artifactIdentity(path) {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > 256 * 1024 * 1024) throw new Error("invalid artifact file");
  return { sha256: await hashFile(path), bytes: metadata.size };
}

export async function copyPinnedArtifact(source, destination, expected) {
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination, constants.COPYFILE_EXCL);
  const copied = await artifactIdentity(destination);
  if (copied.sha256 !== expected.sha256 || copied.bytes !== expected.bytes) throw new Error("copied artifact identity mismatch");
}

export function validatePlan(plan) {
  const fields = ["test_only", "build_dir", "output_dir", "base_url", "chain_id", "settlement_contract", "verifier_version"];
  if (Object.keys(plan).some((key) => !fields.includes(key)) || fields.some((key) => !(key in plan))) throw new Error("invalid plan fields");
  if (plan.test_only !== true) throw new Error("only test-only bundles are supported; independent ceremony approval is required for release keys");
  for (const key of ["chain_id", "verifier_version"]) {
    if (!Number.isSafeInteger(plan[key]) || plan[key] <= 0) throw new Error(`invalid ${key}`);
  }
  if (!/^0x[0-9a-f]{40}$/.test(plan.settlement_contract) || /^0x0{40}$/.test(plan.settlement_contract)) throw new Error("invalid settlement_contract");
  const url = new URL(plan.base_url);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash || !url.pathname.endsWith("/")) throw new Error("invalid base_url");
  for (const key of ["build_dir", "output_dir"]) if (typeof plan[key] !== "string" || !plan[key]) throw new Error(`invalid ${key}`);
  return plan;
}

export async function sourceInventory(circuitRoot = root) {
  const base = await realpath(circuitRoot);
  const visited = new Set();
  async function visit(path) {
    const actual = await realpath(path);
    if (!actual.startsWith(`${base}${sep}`)) throw new Error("source include escapes circuit root");
    if (visited.has(actual)) return;
    visited.add(actual);
    const text = await readFile(actual, "utf8");
    // Circom includes use quoted literal paths; remove comments before resolving them.
    const code = text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
    for (const match of code.matchAll(/\binclude\s+"([^"]+)"\s*;/g)) {
      let included;
      for (const candidate of [resolve(dirname(actual), match[1]), resolve(base, "node_modules", match[1]), resolve(base, "lib", match[1])]) {
        try { included = await realpath(candidate); break; } catch (error) { if (error.code !== "ENOENT") throw error; }
      }
      if (!included) throw new Error(`missing include ${match[1]}`);
      await visit(included);
    }
  }
  for (const kind of kinds) await visit(resolve(base, `${kind}.circom`));
  const entries = [];
  for (const path of [...visited].sort()) entries.push({ path: relative(base, path).split(sep).join("/"), sha256: await hashFile(path) });
  return { version: 1, files: entries };
}

export async function buildBundle(rawPlan) {
  const plan = validatePlan(rawPlan);
  const build = resolve(plan.build_dir);
  const output = resolve(plan.output_dir);
  const inventory = await sourceInventory();
  const pins = new Map();
  for (const kind of kinds) {
    for (const name of [`${kind}_js/${kind}.wasm`, `${kind}.r1cs`, `${kind}.zkey`, `${kind}-verification-key.json`]) {
      pins.set(name, await artifactIdentity(resolve(build, name)));
    }
  }
  pins.set("pot16_final.ptau", await artifactIdentity(resolve(build, "pot16_final.ptau")));
  const circomBinary = await realpath(execFileSync("which", ["circom"], { encoding: "utf8" }).trim());
  const solcEngine = createRequire(import.meta.url).resolve("solc/soljson.js");
  const toolPins = new Map();
  for (const [label, path] of [["circom_binary", circomBinary], ["node_binary", process.execPath], ["solc_engine", solcEngine]]) {
    toolPins.set(label, { path, sha256: await hashFile(path) });
  }
  const circomVersion = execFileSync(circomBinary, ["--version"], { encoding: "utf8" }).trim();
  if (circomVersion !== "circom compiler 2.2.3") throw new Error("requires pinned circom 2.2.3");
  const rebuilt = await mkdtemp(resolve(tmpdir(), "erebus-m5-rebuild-"));
  try {
    for (const kind of kinds) {
      execFileSync(circomBinary, [`${kind}.circom`, "--r1cs", "--wasm", "-l", "node_modules", "-l", "lib", "-o", rebuilt], { cwd: root, stdio: "pipe" });
      for (const name of [`${kind}.r1cs`, `${kind}_js/${kind}.wasm`]) {
        if (await hashFile(resolve(rebuilt, name)) !== await hashFile(resolve(build, name))) throw new Error(`source rebuild mismatch: ${name}`);
      }
    }
  } finally { await rm(rebuilt, { recursive: true, force: true }); }
  // Run the existing consistency gate before creating any output. No setup is generated.
  execFileSync(process.execPath, [resolve(root, "scripts/verify-setup.mjs"), build], { stdio: "inherit" });
  const toolchain = {
    node: process.version, circom: circomVersion,
    executable_sha256: Object.fromEntries([...toolPins].map(([label, record]) => [label, record.sha256])),
    solc: solc.version(), package_lock_sha256: await hashFile(resolve(root, "package-lock.json")),
    bundle_builder_sha256: await hashFile(fileURLToPath(import.meta.url)),
    setup_checker_sha256: await hashFile(resolve(root, "scripts/verify-setup.mjs")),
    compiler_settings: { optimizer: { enabled: false, runs: 200 }, evmVersion: "paris" },
    ptau_sha256: pins.get("pot16_final.ptau").sha256,
    source_rebuild_matches: true,
    test_only: true, provenance: "Test-only input keys. Source rebuild and setup consistency do not establish independent contributions, secret destruction, or an audit.",
  };
  if (!toolchain.solc.startsWith("0.8.24+")) throw new Error("requires pinned solc 0.8.24");
  inventory.toolchain = toolchain;
  inventory.witness_sources = [];
  for (const name of ["sdk/core/src/shielded.rs", "sdk/core/src/shielded_auth.rs", "sdk/shielded/src/lib.rs", "circuits/m5/scripts/pool.mjs", "circuits/m5/scripts/native-fixture.mjs"]) {
    inventory.witness_sources.push({ path: name, sha256: await hashFile(resolve(root, "../..", name)) });
  }
  const inventoryBytes = json(inventory);
  const sourceHash = createHash("sha256").update(inventoryBytes).digest("hex");
  const circuits = [];
  const generated = [];
  for (const kind of kinds) {
    const zkey = resolve(build, `${kind}.zkey`);
    const source = await snarkjs.zKey.exportSolidityVerifier(zkey, { groth16: await readFile(resolve(root, "node_modules/snarkjs/templates/verifier_groth16.sol.ejs"), "utf8") });
    const compiled = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity", sources: { "verifier.sol": { content: source } }, settings: { ...toolchain.compiler_settings, outputSelection: { "*": { "*": ["evm.deployedBytecode.object"] } } } })));
    if (compiled.errors?.some((error) => error.severity === "error")) throw new Error(`verifier compilation failed: ${kind}`);
    const runtime = compiled.contracts?.["verifier.sol"]?.Groth16Verifier?.evm.deployedBytecode.object;
    if (!runtime) throw new Error(`missing verifier runtime: ${kind}`);
    generated.push([`${kind}-verifier.sol`, source], [`${kind}-runtime.hex`, `${runtime}\n`]);
    const record = { kind };
    for (const [field, name] of [["wasm", `${kind}_js/${kind}.wasm`], ["r1cs", `${kind}.r1cs`], ["zkey", `${kind}.zkey`]]) {
      record[field] = { url: new URL(name, plan.base_url).href, ...pins.get(name) };
    }
    record.identity = { source_inventory_sha256: sourceHash, witness_schema_version: 2, verification_key_sha256: pins.get(`${kind}-verification-key.json`).sha256, verifier_runtime_keccak256: keccak256(`0x${runtime}`).slice(2) };
    circuits.push(record);
  }
  // Existing output is never overwritten; failed output must not be treated as qualified.
  await mkdir(output);
  for (const [name, pin] of pins) await copyPinnedArtifact(resolve(build, name), resolve(output, name), pin);
  for (const [name, bytes] of generated) await writeFile(resolve(output, name), bytes, { flag: "wx" });
  await writeFile(resolve(output, "source-inventory.json"), inventoryBytes, { flag: "wx" });
  await writeFile(resolve(output, "toolchain.json"), json(toolchain), { flag: "wx" });
  const currentInventory = await sourceInventory();
  if (JSON.stringify(currentInventory.files) !== JSON.stringify(inventory.files)) throw new Error("circuit sources changed during build");
  for (const record of inventory.witness_sources) {
    if (await hashFile(resolve(root, "../..", record.path)) !== record.sha256) throw new Error("witness sources changed during build");
  }
  for (const [name, pin] of pins) {
    const current = await artifactIdentity(resolve(build, name));
    if (current.sha256 !== pin.sha256 || current.bytes !== pin.bytes) throw new Error("artifact input changed during build");
  }
  for (const pin of toolPins.values()) {
    if (await hashFile(pin.path) !== pin.sha256) throw new Error("toolchain executable changed during build");
  }
  const manifest = { version: 2, chain_id: plan.chain_id, settlement_contract: plan.settlement_contract, verifier_version: plan.verifier_version, test_only: true, circuits };
  await writeFile(resolve(output, "manifest.json"), json(manifest), { flag: "wx" });
  return { test_only: true, manifest_sha256: await hashFile(resolve(output, "manifest.json")), source_inventory_sha256: sourceHash };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error("usage: node build-artifact-bundle.mjs <plan.json>");
    console.log(JSON.stringify(await buildBundle(JSON.parse(await readFile(process.argv[2], "utf8")))));
    process.exit(0);
  } catch (error) { console.error(error.message); process.exit(1); }
}
