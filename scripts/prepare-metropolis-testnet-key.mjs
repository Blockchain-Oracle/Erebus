// Generates an owner-only testnet gas key; never signs or broadcasts transactions.
import { createECDH, randomBytes } from "node:crypto";
import { lstat, mkdir, open, realpath } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export async function prepareTestnetKey(path, binary) {
  path = resolve(path);
  const parent = dirname(path);
  await mkdir(parent, { recursive: true, mode: 0o700 });
  const metadata = await lstat(parent);
  if (!metadata.isDirectory() || await realpath(parent) !== parent || metadata.uid !== process.getuid()
      || (metadata.mode & 0o077) !== 0) throw new Error("requires an owned private directory");
  // Exclusive creation refuses replacement of an existing key or symbolic link.
  const file = await open(path, "wx", 0o600);
  const secret = randomBytes(32);
  try {
    const curve = createECDH("secp256k1");
    curve.setPrivateKey(secret);
    await file.writeFile(secret);
    await file.sync();
  } finally { secret.fill(0); await file.close(); }
  const directory = await open(parent, "r");
  try { await directory.sync(); } finally { await directory.close(); }
  const result = JSON.parse(execFileSync(resolve(binary), [], {
    input: JSON.stringify({ method: "address", key_file: path }),
    encoding: "utf8", timeout: 10000, stdio: ["pipe", "pipe", "pipe"],
  }));
  if (result.status !== "ok" || !/^0x[0-9a-f]{40}$/.test(result.address)) throw new Error("address derivation failed");
  return { testnet_only: true, chain_id: 10143, address: result.address, key_file: path,
    transactions_sent: 0 };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 4) throw new Error("invalid arguments");
    console.log(JSON.stringify(await prepareTestnetKey(process.argv[2], process.argv[3])));
  } catch { console.error("testnet key preparation failed; existing keys are never replaced"); process.exitCode = 1; }
}
