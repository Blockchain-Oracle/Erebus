// Regression using local prototype artifacts; never alters the original build or certifies trust.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, symlinkSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const build = resolve(process.argv[2] ?? fileURLToPath(new URL("../build/", import.meta.url)));
const checker = fileURLToPath(new URL("./verify-setup.mjs", import.meta.url));
const temporary = mkdtempSync(join(tmpdir(), "erebus-setup-check-"));
try {
  symlinkSync(join(build, "pot16_final.ptau"), join(temporary, "pot16_final.ptau"));
  for (const circuit of ["deposit", "transfer", "withdraw"]) {
    for (const suffix of ["r1cs", "zkey"]) {
      symlinkSync(join(build, `${circuit}.${suffix}`), join(temporary, `${circuit}.${suffix}`));
    }
    const name = `${circuit}-verification-key.json`;
    const key = JSON.parse(readFileSync(join(build, name), "utf8"));
    if (circuit === "deposit") key.nPublic += 1;
    writeFileSync(join(temporary, name), JSON.stringify(key));
  }
  const result = spawnSync(process.execPath, [checker, temporary], {
    encoding: "utf8", timeout: 180_000, maxBuffer: 1024 * 1024,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 1, result.stderr);
  const records = result.stdout.split("\n").filter((line) => line.startsWith("{")).map(JSON.parse);
  assert.equal(records.find((record) => record.circuit === "deposit").zkeyVerifies, true);
  assert.equal(records.find((record) => record.circuit === "deposit").verificationKeyMatches, false);
  for (const circuit of ["transfer", "withdraw"]) {
    assert.equal(records.find((record) => record.circuit === circuit).verificationKeyMatches, true);
  }
  console.log("Tampered verification key rejected; unchanged setup files preserved.");
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
