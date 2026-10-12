import assert from "node:assert/strict";
import { parseWalletReply, parseSyncOptions } from "./sync-live-wallet.mjs";

assert.equal(parseSyncOptions([]).maxSteps, 100);
assert.equal(parseSyncOptions(["--max-steps", "1"]).maxSteps, 1);
assert.equal(parseSyncOptions(["--wallet-binary", "/tmp/wallet"]).binary, "/tmp/wallet");
for (const args of [["--max-steps", "0"], ["--max-steps", "101"], ["--max-steps", "1.5"],
  ["--max-steps", "-1"], ["--wallet-binary"], ["--unknown", "1"],
  ["--max-steps", "1", "--max-steps", "2"]]) assert.throws(() => parseSyncOptions(args));

assert.equal(parseWalletReply(0, '{"status":"ok","result":{}}', "").status, "ok");
assert.equal(parseWalletReply(2, "", '{"status":"pending","error":"history incomplete"}').status, "pending");
assert.equal(parseWalletReply(1, "", '{"status":"error","error":"paired evidence unavailable"}').status, "error");
assert.throws(() => parseWalletReply(2, '{"status":"pending"}', ""));
assert.throws(() => parseWalletReply(2, "", '{"status":"ok"}'));
assert.throws(() => parseWalletReply(0, '{"status":"pending"}', ""));
assert.throws(() => parseWalletReply(null, "", '{"status":"pending"}'));
assert.throws(() => parseWalletReply(2, "", "not JSON"));
console.log("wallet sync reply-channel and fail-closed parsing tests passed; no RPC used");
