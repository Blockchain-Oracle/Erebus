/**
 * Reading a discovery index in the browser: the file `discovery_demo.py` writes
 * (agents/src/erebus_agents/discovery.py). Records are pointers to signed descriptors, not proof.
 * Nothing here verifies a descriptor's signature; Rust does that when an operator configures the
 * file as its peer descriptor. What the browser can check is that a descriptor file still holds
 * the exact bytes a record pinned.
 */

export const MODE_PUBLIC_BOUND = 0b01;
export const MODE_SHIELDED = 0b10;

export type IndexRecord = {
  serviceId: string;
  capability: string;
  minUnitPrice: number;
  maxUnitPrice: number;
  descriptorFile: string;
  descriptorSha256: string;
  sellerAddress: string;
  endpoints: string[];
  chainNamespace: string;
  assets: string[];
  settlementModes: number;
  expires: number;
};

export class IndexError extends Error {}

const SERVICE_ID = /^[a-z0-9][a-z0-9._-]*$/;
const SHA256 = /^[a-f0-9]{64}$/;
const ADDRESS = /^[a-f0-9]{40}$/;

const isStrings = (value: unknown): value is string[] =>
  Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === "string" && item.length > 0);
const isCount = (value: unknown): value is number => typeof value === "number" && Number.isInteger(value) && value > 0;

function parseRecord(value: unknown, index: number): IndexRecord {
  const where = `record ${index + 1}`;
  if (typeof value !== "object" || value === null) throw new IndexError(`${where} is not an object.`);
  const r = value as Record<string, unknown>;
  if (typeof r.service_id !== "string" || !SERVICE_ID.test(r.service_id)) throw new IndexError(`${where} has no valid service_id.`);
  if (typeof r.capability !== "string" || !r.capability) throw new IndexError(`${where} has no capability.`);
  if (!isCount(r.min_unit_price) || !isCount(r.max_unit_price) || r.min_unit_price > r.max_unit_price) {
    throw new IndexError(`${where} has an invalid price band.`);
  }
  if (typeof r.descriptor_sha256 !== "string" || !SHA256.test(r.descriptor_sha256)) {
    throw new IndexError(`${where} has no pinned descriptor digest.`);
  }
  if (typeof r.seller_address !== "string" || !ADDRESS.test(r.seller_address)) throw new IndexError(`${where} has no valid seller address.`);
  if (!isStrings(r.endpoints) || !isStrings(r.assets)) throw new IndexError(`${where} has no endpoints or assets.`);
  if (typeof r.chain_namespace !== "string" || !r.chain_namespace) throw new IndexError(`${where} names no chain.`);
  if (!isCount(r.settlement_modes) || r.settlement_modes & ~(MODE_PUBLIC_BOUND | MODE_SHIELDED)) {
    throw new IndexError(`${where} has unknown settlement modes.`);
  }
  if (!isCount(r.expires)) throw new IndexError(`${where} has no expiry.`);
  return {
    serviceId: r.service_id,
    capability: r.capability,
    minUnitPrice: r.min_unit_price,
    maxUnitPrice: r.max_unit_price,
    descriptorFile: typeof r.descriptor_file === "string" ? r.descriptor_file : "",
    descriptorSha256: r.descriptor_sha256,
    sellerAddress: r.seller_address,
    endpoints: r.endpoints,
    chainNamespace: r.chain_namespace,
    assets: r.assets,
    settlementModes: r.settlement_modes,
    expires: r.expires,
  };
}

export function parseIndex(document: unknown): IndexRecord[] {
  const records = (document as { records?: unknown } | null)?.records;
  if (!Array.isArray(records)) throw new IndexError("Expected a discovery index: {\"records\": [...]}.");
  const parsed = records.map(parseRecord);
  const seen = new Set<string>();
  for (const record of parsed) {
    if (seen.has(record.serviceId)) throw new IndexError(`service_id ${record.serviceId} appears twice.`);
    seen.add(record.serviceId);
  }
  return parsed;
}

export type Filters = { capability: string; asset: string; maxUnitPrice: string; mode: "" | "public-bound" | "shielded"; showExpired: boolean };

export const modeLabels = (modes: number) =>
  [modes & MODE_SHIELDED ? "shielded" : "", modes & MODE_PUBLIC_BOUND ? "public-bound" : ""].filter(Boolean);

/** Cheapest first, ties on service_id: the same order `DiscoveryDirectory.search` returns. */
export function search(records: IndexRecord[], filters: Filters, now: number): IndexRecord[] {
  const ceiling = filters.maxUnitPrice.trim() === "" ? null : Number(filters.maxUnitPrice);
  const mode = filters.mode === "shielded" ? MODE_SHIELDED : filters.mode === "public-bound" ? MODE_PUBLIC_BOUND : 0;
  return records
    .filter(
      (r) =>
        (!filters.capability || r.capability === filters.capability) &&
        (filters.showExpired || now < r.expires) &&
        (!filters.asset.trim() || r.assets.includes(filters.asset.trim())) &&
        (ceiling === null || (Number.isFinite(ceiling) && r.minUnitPrice <= ceiling)) &&
        (!mode || (r.settlementModes & mode) !== 0),
    )
    .sort((a, b) => a.minUnitPrice - b.minUnitPrice || a.serviceId.localeCompare(b.serviceId));
}

export async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** Clearly synthetic, so a first-time visitor can see the tool work. Not real sellers. */
export const EXAMPLE_INDEX = {
  records: [
    { service_id: "svc-atlas", capability: "summarize-documents", min_unit_price: 3, max_unit_price: 7,
      descriptor_file: "/example/atlas/seller.descriptor.json", descriptor_sha256: "a1".repeat(32),
      seller_address: "11".repeat(20), endpoints: ["tcp://atlas.example:7443"], chain_namespace: "eip155:10143",
      assets: ["eip155:10143/erc20:0x00000000000000000000000000000000000000aa"], settlement_modes: 2, expires: 4102444800 },
    { service_id: "svc-aegean", capability: "summarize-documents", min_unit_price: 4, max_unit_price: 9,
      descriptor_file: "/example/aegean/seller.descriptor.json", descriptor_sha256: "b2".repeat(32),
      seller_address: "22".repeat(20), endpoints: ["tcp://aegean.example:7443"], chain_namespace: "eip155:10143",
      assets: ["eip155:10143/erc20:0x00000000000000000000000000000000000000aa"], settlement_modes: 3, expires: 4102444800 },
    { service_id: "svc-boreas", capability: "run-model", min_unit_price: 6, max_unit_price: 12,
      descriptor_file: "/example/boreas/seller.descriptor.json", descriptor_sha256: "c3".repeat(32),
      seller_address: "33".repeat(20), endpoints: ["tcp://boreas.example:7443"], chain_namespace: "eip155:10143",
      assets: ["eip155:10143/erc20:0x00000000000000000000000000000000000000bb"], settlement_modes: 1, expires: 1700000000 },
  ],
};
