import assert from "node:assert/strict";
import { Wallet } from "ethers";
import { validateSignedFunding, fundOperators } from "./fund-live-operators.mjs";
const wallet = Wallet.createRandom();
const recipient = Wallet.createRandom().address;
const fields = { type: 2, chainId: 10143, nonce: 7, to: recipient, value: 2_000_000_000_000_000_000n,
  data: "0x", gasLimit: 25201, maxFeePerGas: 300000000000n, maxPriorityFeePerGas: 2000000000n };
const raw = await wallet.signTransaction(fields);
assert.match(validateSignedFunding(raw, wallet.address, recipient, 7), /^0x[0-9a-f]{64}$/);
for (const patch of [{ chainId: 1 }, { nonce: 8 }, { to: wallet.address }, { value: 1n }, { data: "0x00" }, { type: 1, gasPrice: 300000000000n, maxFeePerGas: undefined, maxPriorityFeePerGas: undefined }]) {
  const changed = await wallet.signTransaction({ ...fields, ...patch });
  assert.throws(() => validateSignedFunding(changed, wallet.address, recipient, 7), /intent mismatch/);
}
assert.throws(() => validateSignedFunding(raw, recipient, recipient, 7), /intent mismatch/);
await assert.rejects(fundOperators("not-read", "not-read", "not-created", false), /authorization/);
console.log("native testnet funding signature/intent binding and authorization tests passed; no RPC used");
