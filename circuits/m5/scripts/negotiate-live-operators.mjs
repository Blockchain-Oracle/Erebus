// Independent installed buyer and seller processes; no chain writes.
import { readFile, open } from "node:fs/promises";
import { resolve } from "node:path";
import { spawn } from "node:child_process";

async function retain(path, value) {
  const file = await open(path, "wx", 0o600);
  try { await file.writeFile(`${JSON.stringify(value)}\n`); await file.sync(); } finally { await file.close(); }
}
function start(binary, cwd, request) {
  const child = spawn(binary, [], { cwd, stdio: ["pipe", "pipe", "pipe"] });
  let out = "";
  child.stdout.on("data", (chunk) => { out += chunk; });
  child.stderr.resume();
  child.stdin.end(JSON.stringify(request));
  return { child, result: new Promise((done) => {
    child.once("error", () => done({ status: "error", error: "process unavailable" }));
    child.once("close", (code) => {
      try { const reply = JSON.parse(out); done(code === 0 ? reply : { status: "error", error: reply.error }); }
      catch { done({ status: "error", error: "process output unavailable" }); }
    });
  }) };
}
try {
  if (process.argv.length !== 3) throw new Error("new prepared workdir required");
  const root = resolve(process.argv[2]);
  const run = JSON.parse(await readFile(resolve(root, "run.json"), "utf8"));
  const binary = resolve(run.binaries, "erebus-negotiate");
  const request = (role) => ({ method: "negotiate", config_file: resolve(root, role, "negotiate.json"), operation_ref: run.operation_ref });
  const seller = start(binary, root, request("seller"));
  const buyer = start(binary, root, request("buyer"));
  const timer = setTimeout(() => { seller.child.kill(); buyer.child.kill(); }, 90_000);
  try {
    const sellerResult = await seller.result;
    const buyerResult = await buyer.result;
    if (sellerResult.status === "error" || buyerResult.status === "error") {
      console.error(JSON.stringify({ seller_error: sellerResult.error, buyer_error: buyerResult.error }));
      throw new Error("negotiation stopped; retain state");
    }
    if (sellerResult.deal_commitment !== buyerResult.deal_commitment || !buyerResult.deal_commitment) throw new Error("agreement mismatch");
    await retain(resolve(root, "seller/negotiated.json"), sellerResult);
    await retain(resolve(root, "buyer/negotiated.json"), buyerResult);
    console.log(JSON.stringify({ status: "negotiated", deal_commitment: buyerResult.deal_commitment,
      two_processes: true, live_transactions_sent: 0 }));
  } finally { clearTimeout(timer); seller.child.kill(); buyer.child.kill(); }
} catch { console.error("negotiation failed; retain state; no payment requested"); process.exitCode = 1; }
