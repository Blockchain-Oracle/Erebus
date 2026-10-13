// Single-operator test setup. Public records only; entropy stays in contribution children.
import { randomBytes } from "node:crypto";
import { constants } from "node:fs";
import { chmod, copyFile, lstat, mkdir, mkdtemp, open, readFile, realpath, writeFile } from "node:fs/promises";
import { fork } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import * as snarkjs from "snarkjs";
import { hashFile, sourceInventory } from "./build-artifact-bundle.mjs";
import { publishedDigest } from "./verify-phase1.mjs";
import { contributionHistory } from "./verify-phase2-step.mjs";
import { validatePlan as validateChain, verifyChain } from "./verify-phase2-chain.mjs";

const self = fileURLToPath(import.meta.url);
const root = fileURLToPath(new URL("../", import.meta.url));
export const kinds = Object.freeze(["deposit", "transfer", "withdraw"]);
export const contributors = Object.freeze(["agent-worker-1", "agent-worker-2"]);
export const candidatePins = Object.freeze({
  deposit: "f5e33b26641fb2e1c60b02258556afa718ed65805ad9372cbf35b1dba8510b69",
  transfer: "4085c95caea10f580b18a1a9989823bad4d52ff5893e26a3a7f2827021040b95",
  withdraw: "34738cc8fbb7f672b8828f2451a405c4e4435dbbbd032ad3aaea6771a1c06a76",
});
const ptauPath = "/private/tmp/erebus-phase1-public-16.ptau";
const ptauHash = "1c401abb57c9ce531370f3015c3e75c0892e0f32b8b1e94ace0f6682d9695922";
const phase1ReceiptPath = "/private/tmp/erebus-phase1-public-receipt.json";
const phase1ReceiptHash = "a54c8ad0e9272723e1324f1cbe5e976c1c8420be84f1e79b37b38c0747ea2132";
const publishedPin = "6a6277a2f74e1073601b4f9fed6e1e55226917efb0f0db8a07d98ab01df1ccf43eb0e8c3159432acd4960e2f29fe84a4198501fa54c8dad9e43297453efec125";
const json = (value) => `${JSON.stringify(value, null, 2)}\n`;
const progress = (stage) => console.error(JSON.stringify({ test_only: true, stage }));

export async function pinFile(path, expected, limit = 256 * 1024 ** 2) {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.size === 0 || metadata.size > limit) throw new Error("invalid public input file");
  const sha256 = await hashFile(path);
  if (expected && sha256 !== expected) throw new Error("public input hash mismatch");
  return { path: resolve(path), sha256 };
}

export async function writePublic(path, value) {
  if (value?.test_only !== true) throw new Error("public output requires test_only:true");
  await writeFile(path, json(value), { flag: "wx", mode: 0o600 });
  return pinFile(path);
}

export async function createOutputDirectory(path) {
  if (path === undefined) return mkdtemp("/private/tmp/erebus-testnet-setup-");
  const absolute = resolve(path);
  if (dirname(absolute) !== "/private/tmp" || !/^erebus-testnet-setup-[A-Za-z0-9_-]+$/.test(absolute.split("/").at(-1))) {
    throw new Error("output must be a fresh erebus-testnet-setup-* directory directly in /private/tmp");
  }
  await mkdir(absolute, { mode: 0o700 });
  return absolute;
}

export async function validateInputs(build = resolve(root, "build")) {
  const packageRecord = await pinFile(resolve(root, "node_modules/snarkjs/package.json"));
  if (JSON.parse(await readFile(packageRecord.path, "utf8")).version !== "0.7.6") throw new Error("requires snarkjs 0.7.6");
  const manifest = await pinFile(resolve(build, "artifact-manifest.json"), undefined, 65536);
  const declared = JSON.parse(await readFile(manifest.path, "utf8"));
  const circuits = [];
  for (const kind of kinds) {
    if (declared.circuits?.[kind]?.r1csSha256 !== candidatePins[kind]) throw new Error("candidate manifest pin mismatch");
    circuits.push({ kind, r1cs: await pinFile(resolve(build, `${kind}.r1cs`), candidatePins[kind]) });
  }
  const ptau = await pinFile(ptauPath, ptauHash);
  const phase1Receipt = await pinFile(phase1ReceiptPath, phase1ReceiptHash, 65536);
  const receipt = JSON.parse(await readFile(phase1Receipt.path, "utf8"));
  if (receipt.consistent !== true || receipt.ptau_sha256 !== ptau.sha256
      || receipt.published_pin?.algorithm !== "blake2b512" || receipt.published_pin.digest !== publishedPin
      || await publishedDigest(ptau.path, "blake2b512") !== publishedPin) throw new Error("verified public phase-1 receipt mismatch");
  const provenance = await pinFile("/private/tmp/erebus-phase1-public-provenance.txt", receipt.provenance_record_sha256, 1048576);
  const scripts = [];
  for (const path of [self, resolve(root, "scripts/verify-phase2-chain.mjs"), resolve(root, "scripts/verify-phase2-step.mjs"),
    resolve(root, "scripts/verify-phase1.mjs"), resolve(root, "scripts/build-artifact-bundle.mjs"),
    resolve(root, "package-lock.json"), packageRecord.path, resolve(root, "node_modules/snarkjs/src/zkey_new.js"),
    resolve(root, "node_modules/snarkjs/src/zkey_contribute.js"), resolve(root, "node_modules/snarkjs/src/misc.js"), process.execPath]) {
    scripts.push(await pinFile(path));
  }
  const sources = await sourceInventory(root);
  return { test_only: true, circuits, ptau, phase1Receipt, provenance, manifest, scripts,
    sources: sources.files.map((record) => ({ path: resolve(root, record.path), sha256: record.sha256 })),
    toolchain: { test_only: true, node: process.version, snarkjs: "0.7.6" } };
}

export function validateWorkerJob(job) {
  if (!job || job.test_only !== true || !["initial", "contribute", "verify", "vkey"].includes(job.action)) throw new Error("invalid test worker job");
  const fields = { initial: ["r1cs", "ptau", "output"], contribute: ["before", "contributor", "output"],
    verify: ["plan"], vkey: ["before", "output"] }[job.action];
  const allowed = ["action", "test_only", ...fields];
  if (Object.keys(job).sort().join(",") !== allowed.sort().join(",")) throw new Error("invalid worker fields");
  for (const key of ["r1cs", "ptau", "before"].filter((key) => fields.includes(key))) {
    const record = job[key];
    if (!record || Object.keys(record).sort().join(",") !== "path,sha256" || typeof record.path !== "string"
        || !/^[0-9a-f]{64}$/.test(record.sha256)) throw new Error("invalid worker file pin");
  }
  if (job.action === "contribute" && !contributors.includes(job.contributor)) throw new Error("invalid test contributor");
  if (job.action === "verify") {
    validateChain(job.plan);
    if (job.plan.test_only !== true || JSON.stringify(job.plan.contributors) !== JSON.stringify(contributors)) throw new Error("invalid test chain");
  } else if (typeof job.output !== "string" || dirname(dirname(resolve(job.output))) !== "/private/tmp"
      || !/^erebus-testnet-setup-[A-Za-z0-9_-]+$/.test(dirname(resolve(job.output)).split("/").at(-1))) {
    throw new Error("invalid worker output path");
  }
  return job;
}

export async function executeWorker(rawJob) {
  const job = validateWorkerJob(rawJob);
  for (const key of ["r1cs", "ptau", "before"]) if (job[key]) await pinFile(job[key].path, job[key].sha256);
  if (job.action === "verify") {
    const receipt = await verifyChain(job.plan);
    // Retain the verifier's public consistency evidence without its generic trust prose.
    return { test_only: true, consistent: receipt.consistent, trust_verified: false, independent: false,
      destruction_verified: false, contributors: receipt.contributors,
      ptau_sha256: receipt.ptau_sha256, circuits: receipt.circuits.map((circuit) => ({ test_only: true,
        kind: circuit.kind, r1cs_sha256: circuit.r1cs_sha256, final_zkey_sha256: circuit.final_zkey_sha256,
        steps: circuit.steps.map((step) => ({ test_only: true, consistent: step.consistent,
          before_sha256: step.before_sha256, after_sha256: step.after_sha256,
          contribution: step.contribution, contribution_count: step.contribution_count })) })) };
  }
  const parent = await lstat(dirname(job.output));
  if (!parent.isDirectory() || await realpath(dirname(job.output)) !== dirname(resolve(job.output))
      || parent.uid !== process.getuid() || (parent.mode & 0o777) !== 0o700) throw new Error("worker requires owned private output directory");
  if (job.action === "vkey") {
    const key = await snarkjs.zKey.exportVerificationKey(job.before.path);
    await pinFile(job.before.path, job.before.sha256);
    await writeFile(job.output, json(key), { flag: "wx", mode: 0o600 });
    const output = await pinFile(job.output);
    const sidecar = await writePublic(`${job.output}.test-only.json`, { test_only: true, vkey: output, zkey: job.before });
    return { test_only: true, output, sidecar };
  }
  // Reserve the filename exclusively before snarkjs writes within our private directory.
  const reserved = await open(job.output, "wx", 0o600);
  await reserved.close();
  if (job.action === "initial") {
    if (await snarkjs.zKey.newZKey(job.r1cs.path, job.ptau.path, job.output) === -1) throw new Error("initial key generation failed");
    if ((await contributionHistory(job.output)).contributions.length !== 0) throw new Error("initial key must have zero contributions");
  } else {
    const entropy = randomBytes(64);
    try {
      await snarkjs.zKey.contribute(job.before.path, job.output, job.contributor, entropy.toString("hex"));
    } finally { entropy.fill(0); }
  }
  for (const key of ["r1cs", "ptau", "before"]) if (job[key]) await pinFile(job[key].path, job[key].sha256);
  return { test_only: true, output: await pinFile(job.output) };
}

export function runWorker(job, { timeoutMs = job.action === "verify" ? 45 * 60 * 1000 : 20 * 60 * 1000 } = {}) {
  validateWorkerJob(job);
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 60 * 60 * 1000) throw new Error("invalid worker timeout");
  return new Promise((resolvePromise, reject) => {
    // No entropy, inherited debugger flags, environment secrets, or child logs cross this boundary.
    const child = fork(self, ["--worker"], { execArgv: [], env: {}, stdio: ["ignore", "ignore", "ignore", "ipc"] });
    let result;
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, timeoutMs);
    child.on("message", (message) => { result = message; });
    child.on("error", () => { clearTimeout(timer); reject(new Error("test setup child failed to start")); });
    child.on("exit", (code) => {
      clearTimeout(timer);
      if (timedOut) reject(new Error(`test setup ${job.action} child timed out`));
      else if (code !== 0 || result?.test_only !== true || result.failed) reject(new Error(`test setup ${job.action} child failed`));
      else resolvePromise(result);
    });
    child.send(job, (error) => { if (error) child.kill(); });
  });
}

export async function runSetup({ build, output } = {}) {
  const inputs = await validateInputs(build);
  const directory = await createOutputDirectory(output);
  console.error(JSON.stringify({ test_only: true, directory, stage: "snapshot inputs" }));
  const inputRecord = await writePublic(resolve(directory, "inputs.json"), inputs);
  const copied = new Map();
  for (const record of [inputs.ptau, ...inputs.circuits.map((circuit) => circuit.r1cs)]) {
    const path = resolve(directory, record === inputs.ptau ? "public-16.ptau" : `${inputs.circuits.find((c) => c.r1cs === record).kind}.r1cs`);
    await copyFile(record.path, path, constants.COPYFILE_EXCL);
    await chmod(path, 0o600);
    copied.set(record.path, await pinFile(path, record.sha256));
  }
  const ptau = copied.get(inputs.ptau.path);
  const plan = { version: 1, test_only: true, contributors: [...contributors], ptau_sha256: ptau.sha256, circuits: [] };
  for (const circuit of inputs.circuits) {
    const { kind } = circuit;
    const r1cs = copied.get(circuit.r1cs.path);
    progress(`${kind}: initial`);
    let before = (await runWorker({ test_only: true, action: "initial", r1cs, ptau, output: resolve(directory, `${kind}-initial.zkey`) })).output;
    const chain = { kind, r1cs_sha256: r1cs.sha256, steps: [] };
    for (let index = 0; index < contributors.length; index++) {
      const contributor = contributors[index];
      progress(`${kind}: ${contributor}`);
      const after = (await runWorker({ test_only: true, action: "contribute", before, contributor,
        output: resolve(directory, `${kind}-${index + 1}.zkey`) })).output;
      chain.steps.push(await writePublic(resolve(directory, `${kind}-step-${index + 1}.json`),
        { version: 1, test_only: true, contributor, r1cs, ptau, before, after }));
      before = after;
    }
    plan.circuits.push(chain);
  }
  const chainRecord = await writePublic(resolve(directory, "chain-plan.json"), plan);
  progress("verifyChain: all three circuits");
  const verification = await runWorker({ test_only: true, action: "verify", plan });
  const verificationRecord = await writePublic(resolve(directory, "chain-verification.json"), verification);
  const vkeys = [];
  for (const circuit of plan.circuits) {
    const step = JSON.parse(await readFile(circuit.steps.at(-1).path, "utf8"));
    progress(`${circuit.kind}: export vkey`);
    const exported = await runWorker({ test_only: true, action: "vkey", before: step.after,
      output: resolve(directory, `${circuit.kind}-verification-key.json`) });
    vkeys.push({ kind: circuit.kind, zkey: step.after, vkey: exported.output, sidecar: exported.sidecar });
  }
  for (const record of [inputs.ptau, inputs.phase1Receipt, inputs.provenance, inputs.manifest,
    ...inputs.scripts, ...inputs.sources, ...inputs.circuits.map((c) => c.r1cs), ...copied.values(), inputRecord, chainRecord, verificationRecord,
    ...vkeys.flatMap((circuit) => [circuit.zkey, circuit.vkey, circuit.sidecar])]) {
    await pinFile(record.path, record.sha256);
  }
  const receipt = { version: 1, test_only: true, operator_count: 1, consistent: true,
    trust_verified: false, independent: false, destruction_verified: false, contributors: [...contributors],
    directory, inputs: inputRecord, chain_plan: chainRecord, chain_verification: verificationRecord, circuits: vkeys };
  const receiptRecord = await writePublic(resolve(directory, "receipt.json"), receipt);
  return { ...receipt, receipt: receiptRecord };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length === 3 && process.argv[2] === "--worker" && process.send) {
    process.once("message", async (job) => {
      try {
        const result = await executeWorker(job);
        process.send(result, () => process.exit(0));
      } catch { process.send({ test_only: true, failed: true }, () => process.exit(1)); }
    });
  } else {
    try {
      if (process.argv.length > 4 || process.argv[2] === "--worker") throw new Error("invalid arguments");
      console.log(JSON.stringify(await runSetup({ build: process.argv[2], output: process.argv[3] })));
      process.exit(0);
    } catch { console.error(JSON.stringify({ test_only: true, failed: true, message: "test setup failed; no completion receipt issued" })); process.exit(1); }
  }
}
