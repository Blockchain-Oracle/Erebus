// Rehearsal amounts are base units, not shared raw integers across assets.
export function liveAssetAmounts(poolName) {
  const decimals = poolName === "wmon" ? 18 : poolName === "usdc" ? 6 : null;
  if (decimals === null) throw new Error("unsupported rehearsal asset");
  const scale = 10n ** BigInt(decimals - 6);
  const units = (microunits) => (BigInt(microunits) * scale).toString();
  return {
    decimals,
    deposit: units(1500),
    price: units(700),
    buyerOffer: units(600),
    minimumPrice: units(650),
    maximumPrice: units(750),
  };
}
