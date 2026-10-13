"use client";

import { useEffect, useMemo, useState } from "react";
import { FileDrop } from "./FileDrop";
import {
  EXAMPLE_INDEX,
  type Filters,
  IndexError,
  type IndexRecord,
  modeLabels,
  parseIndex,
  search,
  sha256Hex,
} from "@/lib/discovery";

const date = (seconds: number) => new Date(seconds * 1000).toISOString().slice(0, 10);

const inputClass =
  "mt-2 w-full border-0 border-b border-rule-2 bg-transparent py-2 text-[13px] text-fore outline-none placeholder:text-fore-3 focus:border-fore";

type Check = { name: string; digest: string; matches: string[] } | { name: string; error: string };

export function DiscoveryTool() {
  const [records, setRecords] = useState<IndexRecord[] | null>(null);
  const [source, setSource] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState<Filters>({ capability: "", asset: "", maxUnitPrice: "", mode: "", showExpired: false });
  const [check, setCheck] = useState<Check | null>(null);
  const [now, setNow] = useState<number | null>(null);

  // The clock is read after mount so the static export never bakes in a build-time "now".
  useEffect(() => setNow(Math.floor(Date.now() / 1000)), []);

  const load = (text: string, name: string) => {
    try {
      setRecords(parseIndex(JSON.parse(text)));
      setError(null);
      setFilters((current) => ({ ...current, capability: "" }));
    } catch (caught) {
      setRecords(null);
      setError(caught instanceof IndexError ? caught.message : "That file is not a discovery index.");
    }
    setSource(name);
    setCheck(null);
  };

  const capabilities = useMemo(
    () => Array.from(new Set((records ?? []).map((r) => r.capability))).sort(),
    [records],
  );
  const found = records && now !== null ? search(records, filters, now) : [];
  const set = <K extends keyof Filters>(key: K, value: Filters[K]) => setFilters((current) => ({ ...current, [key]: value }));

  const checkDescriptor = async (bytes: ArrayBuffer, name: string) => {
    const digest = await sha256Hex(bytes);
    setCheck({ name, digest, matches: (records ?? []).filter((r) => r.descriptorSha256 === digest).map((r) => r.serviceId) });
  };

  return (
    <div>
      <div className="grid grid-cols-1 gap-10 lg:grid-cols-[0.9fr_1.1fr] lg:gap-16">
        <div>
          <FileDrop
            id="index-file"
            accept="application/json,.json"
            prompt="Drop discovery.json here, or choose the file"
            onText={load}
            onError={(message) => {
              setRecords(null);
              setError(message);
            }}
          />
          <div className="mt-4 flex flex-wrap items-center justify-between gap-4">
            <span className="mono-xs text-fore-3">
              {source ? `read: ${source}` : "no index open"}
              {records ? ` · ${records.length} record${records.length === 1 ? "" : "s"}` : ""}
            </span>
            <button type="button" className="link" onClick={() => load(JSON.stringify(EXAMPLE_INDEX), "example (synthetic sellers)")}>
              Load an example
            </button>
          </div>
          {error ? (
            <div className="mt-6 border-t border-rule py-5" aria-live="polite">
              <p className="label m-0 mb-3">Not an index</p>
              <p className="prose m-0">{error}</p>
            </div>
          ) : null}
        </div>

        <fieldset disabled={!records} className="m-0 grid grid-cols-1 gap-x-8 border-0 p-0 sm:grid-cols-2 disabled:opacity-50">
          <legend className="label mb-2">Filter</legend>
          <label className="block border-t border-rule py-4">
            <span className="label block">Capability</span>
            <select value={filters.capability} onChange={(e) => set("capability", e.target.value)} className={inputClass}>
              <option value="">any</option>
              {capabilities.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <label className="block border-t border-rule py-4">
            <span className="label block">Settlement mode</span>
            <select value={filters.mode} onChange={(e) => set("mode", e.target.value as Filters["mode"])} className={inputClass}>
              <option value="">any</option>
              <option value="shielded">shielded</option>
              <option value="public-bound">public-bound</option>
            </select>
          </label>
          <label className="block border-t border-rule py-4">
            <span className="label block">Asset (CAIP-19)</span>
            <input value={filters.asset} onChange={(e) => set("asset", e.target.value)} placeholder="any" spellCheck={false} className={inputClass} />
          </label>
          <label className="block border-t border-rule py-4">
            <span className="label block">Ceiling, unit price</span>
            <input
              value={filters.maxUnitPrice}
              onChange={(e) => set("maxUnitPrice", e.target.value.replace(/[^0-9]/g, ""))}
              inputMode="numeric"
              placeholder="any"
              className={inputClass}
            />
          </label>
          <label className="flex items-center gap-3 border-t border-rule py-4 sm:col-span-2">
            <input type="checkbox" checked={filters.showExpired} onChange={(e) => set("showExpired", e.target.checked)} />
            <span className="mono-xs uppercase tracking-[0.14em] text-fore-2">Include expired descriptors</span>
          </label>
        </fieldset>
      </div>

      {records ? (
        <div className="mt-12" aria-live="polite">
          <p className="label m-0 mb-4">
            {found.length} match{found.length === 1 ? "" : "es"} · cheapest first
          </p>
          <ol className="m-0 list-none p-0">
            {found.map((r) => {
              const expired = now !== null && now >= r.expires;
              return (
                <li key={r.serviceId} className="grid grid-cols-1 gap-4 border-t border-rule py-6 md:grid-cols-[15rem_1fr] md:gap-10">
                  <div>
                    <p className="display m-0 text-[clamp(19px,2vw,27px)] leading-[1.06]">{r.serviceId}</p>
                    <p className="mono-xs m-0 mt-2 uppercase tracking-[0.14em] text-fore-3">{r.capability}</p>
                    <p className="tnum m-0 mt-3 text-[15px] text-fore">
                      {r.minUnitPrice}–{r.maxUnitPrice}
                      <span className="mono-xs ml-2 text-fore-3">advertised, unsigned</span>
                    </p>
                  </div>
                  <dl className="m-0 grid grid-cols-1 gap-x-6 gap-y-2 text-[12px] sm:grid-cols-[8rem_1fr]">
                    <dt className="text-fore-3">modes</dt>
                    <dd className="m-0 text-fore">{modeLabels(r.settlementModes).join(", ")}</dd>
                    <dt className="text-fore-3">chain</dt>
                    <dd className="m-0 break-all text-fore">{r.chainNamespace}</dd>
                    <dt className="text-fore-3">assets</dt>
                    <dd className="m-0 break-all text-fore">{r.assets.join(", ")}</dd>
                    <dt className="text-fore-3">endpoints</dt>
                    <dd className="m-0 break-all text-fore">{r.endpoints.join(", ")}</dd>
                    <dt className="text-fore-3">seller</dt>
                    <dd className="m-0 break-all text-fore">{r.sellerAddress}</dd>
                    <dt className="text-fore-3">expires</dt>
                    <dd className="m-0 text-fore">
                      {date(r.expires)}
                      {expired ? <span className="mono-xs ml-3 uppercase tracking-[0.14em] text-fore-3">expired · resolve refuses it</span> : null}
                    </dd>
                    <dt className="text-fore-3">descriptor</dt>
                    <dd className="m-0 break-all text-fore-2">{r.descriptorFile || "path not recorded"}</dd>
                    <dt className="text-fore-3">pinned sha-256</dt>
                    <dd className="m-0 break-all text-fore-2">{r.descriptorSha256}</dd>
                  </dl>
                </li>
              );
            })}
          </ol>
          {found.length === 0 ? <p className="prose m-0 border-t border-rule py-6">Nothing matches these filters.</p> : null}
        </div>
      ) : null}

      <div className="section-head mt-20">
        <div>
          <h2 className="display m-0 text-[clamp(22px,2.6vw,36px)]">Check a descriptor file.</h2>
          <p className="prose mt-4 max-w-[54ch]">
            Before an operator configures a descriptor as its peer, confirm the file still holds the bytes
            its record pinned. A different digest means the file changed after it was indexed.
          </p>
        </div>
        <div>
          <FileDrop
            id="descriptor-file"
            accept="application/json,.json"
            prompt="Drop a seller.descriptor.json"
            onBytes={(bytes, name) => void checkDescriptor(bytes, name)}
            onError={(message) => setCheck({ name: "", error: message })}
          />
          {check ? (
            <div className="mt-6 border-t border-rule py-5" aria-live="polite">
              {"error" in check ? (
                <p className="prose m-0">{check.error}</p>
              ) : (
                <>
                  <p className="label m-0 mb-3">{check.name}</p>
                  <p className="m-0 break-all text-[12px] text-fore-2">sha-256 {check.digest}</p>
                  <p className="prose m-0 mt-3">
                    {!records
                      ? "Open an index first to compare this file against a pinned record."
                      : check.matches.length
                        ? `Unchanged: these are exactly the bytes ${check.matches.join(", ")} pinned.`
                        : "No record in this index pins these bytes. The file changed since it was indexed, or it was never indexed."}
                  </p>
                </>
              )}
            </div>
          ) : null}
          <p className="mono-xs m-0 mt-4 leading-relaxed text-fore-3">
            A matching digest is not a signature check. Rust verifies the seller&rsquo;s signature when the
            negotiation file loads the descriptor.
          </p>
        </div>
      </div>
    </div>
  );
}
