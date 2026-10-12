import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { lstat, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import test from "node:test";
import { candidatePins, contributors, createOutputDirectory, executeWorker, pinFile,
  runSetup, runWorker, validateInputs, validateWorkerJob, writePublic } from "./testnet-setup.mjs";

const self = fileURLToPath(new URL("./testnet-setup.mjs", import.meta.url));
const exec = promisify(execFile);
const temporary = async (fn) => {
  const directory = await mkdtemp("/private/tmp/erebus-testnet-setup-test-");
  try { await fn(directory); }
  finally { await rm(directory, { recursive: true, force: true }); }
};

test("candidate R1CS and prior verified public transcript match their pins", async () => {
  const inputs = await validateInputs();
  assert.equal(inputs.test_only, true);
  assert.deepEqual(Object.fromEntries(inputs.circuits.map((c) => [c.kind, c.r1cs.sha256])), candidatePins);
  assert.ok(inputs.scripts.some((record) => record.path === process.execPath));
  assert.ok(inputs.scripts.some((record) => record.path.endsWith("package-lock.json")));
  assert.equal(inputs.phase1Receipt.path, "/private/tmp/erebus-phase1-public-receipt.json");
});

test("refuse mismatched, empty, oversized, directory and symlink inputs", async () => temporary(async (directory) => {
  const path = resolve(directory, "public.bin");
  await writeFile(path, "public fixture");
  const record = await pinFile(path);
  await assert.rejects(pinFile(path, "00".repeat(32)), /hash mismatch/);
  await assert.rejects(pinFile(path, record.sha256, 1), /invalid public input/);
  await assert.rejects(pinFile(directory), /invalid public input/);
  const link = resolve(directory, "link");
  await symlink(path, link);
  await assert.rejects(pinFile(link), /invalid public input/);
  await writeFile(path, "");
  await assert.rejects(pinFile(path), /invalid public input/);
}));

test("refuse wrong candidate manifest before creating output", async () => temporary(async (directory) => {
  await writeFile(resolve(directory, "artifact-manifest.json"), JSON.stringify({ circuits: {} }));
  const output = resolve("/private/tmp", `erebus-testnet-setup-refusal-${process.pid}`);
  await assert.rejects(runSetup({ build: directory, output }), /manifest pin mismatch/);
  await assert.rejects(readFile(resolve(output, "inputs.json")), { code: "ENOENT" });
}));

test("output directories are exclusive and constrained to /private/tmp", async () => temporary(async (directory) => {
  await assert.rejects(createOutputDirectory(directory), { code: "EEXIST" });
  await assert.rejects(createOutputDirectory(resolve(directory, "nested")), /fresh/);
  await assert.rejects(createOutputDirectory("/private/tmp/unrelated"), /fresh/);
  const fresh = await createOutputDirectory();
  try {
    assert.equal(fresh.startsWith("/private/tmp/erebus-testnet-setup-"), true);
    assert.equal((await lstat(fresh)).mode & 0o777, 0o700);
    await assert.rejects(createOutputDirectory(fresh), { code: "EEXIST" });
  } finally { await rm(fresh, { recursive: true }); }
}));

test("public JSON requires explicit test_only and cannot overwrite", async () => temporary(async (directory) => {
  const path = resolve(directory, "receipt.json");
  await assert.rejects(writePublic(path, { test_only: false }), /test_only:true/);
  const record = await writePublic(path, { test_only: true, operator_count: 1 });
  const original = await readFile(path, "utf8");
  await assert.rejects(writePublic(path, { test_only: true }), { code: "EEXIST" });
  assert.equal(await readFile(path, "utf8"), original);
  assert.deepEqual(await pinFile(path), record);
}));

test("worker jobs require test labels, exact public fields and pinned records", () => {
  const record = { path: "/private/tmp/public.zkey", sha256: "11".repeat(32) };
  const job = { action: "contribute", test_only: true, before: record,
    contributor: contributors[0], output: "/private/tmp/erebus-testnet-setup-example/after.zkey" };
  assert.equal(validateWorkerJob(job), job);
  for (const changed of [null, { ...job, test_only: false }, { ...job, contributor: "other" },
    { ...job, entropy: "FORBIDDEN_FIELD" }, { ...job, output: "/private/tmp/existing.zkey" },
    { ...job, before: { ...record, sha256: "wrong" } }, { ...job, action: "unknown" }]) {
    assert.throws(() => validateWorkerJob(changed));
  }
});

test("worker refuses an existing zkey without changing it", async () => temporary(async (directory) => {
  const path = resolve(directory, "after.zkey");
  await writeFile(path, "retained public bytes");
  const before = await pinFile(path);
  await assert.rejects(executeWorker({ test_only: true, action: "contribute", before,
    contributor: contributors[1], output: path }), { code: "EEXIST" });
  assert.deepEqual(await pinFile(path), before);
}));

test("worker verifies input pin before touching output", async () => temporary(async (directory) => {
  const path = resolve(directory, "before.zkey");
  await writeFile(path, "retained public bytes");
  const output = resolve(directory, "after.zkey");
  await assert.rejects(executeWorker({ test_only: true, action: "contribute",
    before: { path, sha256: "00".repeat(32) }, contributor: contributors[0], output }), /hash mismatch/);
  await assert.rejects(readFile(output), { code: "ENOENT" });
}));

test("child failure suppresses raw parser exceptions", async () => temporary(async (directory) => {
  const path = resolve(directory, "malformed.zkey");
  const marker = "PRIVATE_EXCEPTION_MARKER";
  await writeFile(path, marker);
  await assert.rejects(runWorker({ test_only: true, action: "contribute", before: await pinFile(path),
    contributor: contributors[0], output: resolve(directory, "after.zkey") }),
  (error) => error.message === "test setup contribute child failed" && !error.message.includes(marker));
}));

test("CLI failures contain only generic test-only JSON", async () => {
  try {
    await exec(process.execPath, [self, "/private/tmp/PRIVATE_INPUT_MARKER"]);
    assert.fail("expected failure");
  } catch (error) {
    assert.equal(error.code, 1);
    assert.equal(error.stdout, "");
    assert.deepEqual(JSON.parse(error.stderr), { test_only: true, failed: true,
      message: "test setup failed; no completion receipt issued" });
    assert.equal(error.stderr.includes("PRIVATE_INPUT_MARKER"), false);
  }
});

test("worker timeout kills and reaps the child with a generic error", async () => temporary(async (directory) => {
  const path = resolve(directory, "before.zkey");
  await writeFile(path, "public invalid key");
  const job = { test_only: true, action: "contribute", before: await pinFile(path),
    contributor: contributors[0], output: resolve(directory, "after.zkey") };
  await assert.rejects(runWorker(job, { timeoutMs: 1 }),
    (error) => error.message === "test setup contribute child timed out");
  assert.throws(() => runWorker(job, { timeoutMs: 0 }), /invalid worker timeout/);
}));

test("vkey export stays canonical with test-only metadata in a sidecar", async () => temporary(async (directory) => {
  const source = fileURLToPath(new URL("../build/deposit.zkey", import.meta.url));
  const output = resolve(directory, "deposit-verification-key.json");
  const before = await pinFile(source);
  const result = await runWorker({ test_only: true, action: "vkey", before, output });
  const key = JSON.parse(await readFile(output, "utf8"));
  const canonical = JSON.parse(await readFile(fileURLToPath(new URL("../build/deposit-verification-key.json", import.meta.url)), "utf8"));
  assert.deepEqual(key, canonical);
  assert.equal("test_only" in key, false);
  assert.equal((await lstat(output)).mode & 0o777, 0o600);
  assert.deepEqual(JSON.parse(await readFile(result.sidecar.path, "utf8")), { test_only: true, vkey: result.output, zkey: before });
  await assert.rejects(runWorker({ test_only: true, action: "vkey", before, output }), /vkey child failed/);
  assert.deepEqual(await pinFile(source), before);
}));
