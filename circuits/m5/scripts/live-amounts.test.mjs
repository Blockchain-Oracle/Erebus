import assert from "node:assert/strict";
import { liveAssetAmounts } from "./live-amounts.mjs";

assert.deepEqual(liveAssetAmounts("wmon"), {
  decimals: 18,
  deposit: "1500000000000000",
  price: "700000000000000",
  buyerOffer: "600000000000000",
  minimumPrice: "650000000000000",
  maximumPrice: "750000000000000",
});
assert.deepEqual(liveAssetAmounts("usdc"), {
  decimals: 6,
  deposit: "1500",
  price: "700",
  buyerOffer: "600",
  minimumPrice: "650",
  maximumPrice: "750",
});
for (const name of ["wmon", "usdc"]) {
  const amounts = liveAssetAmounts(name);
  assert(BigInt(amounts.buyerOffer) < BigInt(amounts.minimumPrice));
  assert(BigInt(amounts.minimumPrice) <= BigInt(amounts.price));
  assert(BigInt(amounts.price) <= BigInt(amounts.maximumPrice));
  assert(BigInt(amounts.maximumPrice) < BigInt(amounts.deposit));
}
for (const name of ["eth", "", null, "constructor", "__proto__"]) {
  assert.throws(() => liveAssetAmounts(name));
}
console.log("live asset base-unit and negotiation bounds tests passed; no RPC or keys used");
