// Read-only catch-up driver. Never requests signing, submission, replacement or a new deal.
import { readFile, mkdir, open } from "node:fs/promises";
import { resolve } from "node:path";
import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

export function parseSyncOptions(args) {
  const options = { binary: null, maxSteps: 100 };
  const seen = new Set();
  for (let i = 0; i < args.length; i += 2) {
    const flag = args[i];
    if (seen.has(flag) || !["--wallet-binary", "--max-steps"].includes(flag) || !args[i + 1]) throw new Error("invalid sync option");
    seen.add(flag);
    if (flag === "--wallet-binary") options.binary = resolve(args[i + 1]);
    else {
      if (!/^[1-9][0-9]*$/.test(args[i + 1])) throw new Error("invalid step bound");
      options.maxSteps = Number(args[i + 1]);
      if (options.maxSteps > 100) throw new Error("step bound exceeds ceiling");
    }
  }
  return options;
}

export function parseWalletReply(code, out, err) {
  if (![0, 1, 2].includes(code)) throw new Error("sync output unavailable");
  const reply = JSON.parse(code === 0 ? out : err);
  const expected = code === 0 ? "ok" : code === 2 ? "pending" : "error";
  if (reply?.status !== expected) throw new Error("sync status disagrees with exit code");
  return reply;
}

function sync(binary, cwd, config) {
  return new Promise((done, fail) => {
    const child = spawn(binary, [], { cwd, stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", (chunk) => { out += chunk; if (out.length > 16 * 1024 * 1024) child.kill(); });
    child.stderr.on("data", (chunk) => { err += chunk; if (err.length > 16 * 1024 * 1024) child.kill(); });
    child.stdin.end(JSON.stringify({ method: "sync", config_file: config }));
    const timer = setTimeout(() => child.kill(), 600_000);
    child.once("error", (error) => { clearTimeout(timer); fail(error); });
    child.once("close", (code) => { clearTimeout(timer); try { done({ code, reply: parseWalletReply(code, out, err) }); } catch { fail(new Error("sync output unavailable")); } });
  });
}
async function main() {
try {
  if (process.argv.length < 4 || !["buyer", "seller"].includes(process.argv[3])) throw new Error("workdir and role required");
  const options = parseSyncOptions(process.argv.slice(4));
  const root = resolve(process.argv[2]);
  const role = process.argv[3];
  const config = resolve(root, role, "wallet.json");
  const run = JSON.parse(await readFile(resolve(root, "run.json"), "utf8"));
  const binary = options.binary ?? resolve(run.binaries, "erebus-wallet");
  const binaryHash = createHash("sha256").update(await readFile(binary)).digest("hex");
  const reports = resolve(root, role, `sync-${Date.now()}`);
  await mkdir(reports, { mode: 0o700 });
  const deadline = Date.now() + 1800_000;
  let complete = false;
  for (let iteration = 0; iteration < options.maxSteps && Date.now() < deadline; iteration++) {
    const result = await sync(binary, root, config);
    const file = await open(resolve(reports, `${iteration}.json`), "wx", 0o600);
    try { await file.writeFile(`${JSON.stringify({ ...result, binary_sha256: binaryHash, source_build_override: options.binary !== null })}\n`); await file.sync(); } finally { await file.close(); }
    const prefixes = {};
    for (const name of ["primary", "peer"]) {
      try {
        const index = JSON.parse(await readFile(resolve(root, role, "state/wallet-index", `${name}.json`), "utf8"));
        prefixes[name] = index.blocks.at(-1)?.number ?? null;
      } catch (error) { if (error.code !== "ENOENT") throw error; prefixes[name] = null; }
    }
    console.log(JSON.stringify({ role, iteration, code: result.code, status: result.reply.status, error: result.reply.error, prefixes, binary_sha256: binaryHash, source_build_override: options.binary !== null }));
    if (result.code === 0) { complete = true; break; }
    if (result.code !== 2 || result.reply.status !== "pending") throw new Error("sync failed closed; retain state");
  }
  if (!complete) { console.error("bounded catch-up remains pending; no transaction sent"); process.exitCode = 2; }
} catch { console.error("wallet catch-up held; no transaction requested; retain state"); process.exitCode = 1; }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
