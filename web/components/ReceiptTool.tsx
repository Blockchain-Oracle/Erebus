"use client";

import { useState } from "react";
import { FileDrop } from "./FileDrop";
import {
  DELIVERY_NOTE,
  EXAMPLE_REPORT,
  PENDING_NOTE,
  type Receipt,
  ReceiptError,
  receiptFromReport,
  receiptHtml,
  receiptJson,
} from "@/lib/receipt";

function download(name: string, type: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

function Field({
  id,
  label,
  value,
  onChange,
  placeholder,
  required = false,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  required?: boolean;
}) {
  return (
    <label htmlFor={id} className="block border-t border-rule py-4">
      <span className="label block">
        {label}
        {required ? <span className="text-fore-2"> · required</span> : null}
      </span>
      <input
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        spellCheck={false}
        autoComplete="off"
        className="mt-2 w-full border-0 border-b border-rule-2 bg-transparent py-2 text-[13px] text-fore outline-none placeholder:text-fore-3 focus:border-fore"
      />
    </label>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-1 gap-1 border-t border-rule py-4 sm:grid-cols-[9rem_1fr] sm:gap-6">
      <span className="label">{label}</span>
      <div className="min-w-0 break-all text-[13px] text-fore">{children}</div>
    </div>
  );
}

const flagText = (flag: boolean) => (flag ? "verified" : "pending");

export function ReceiptTool() {
  const [report, setReport] = useState<unknown>(null);
  const [source, setSource] = useState<string | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [issuer, setIssuer] = useState("");
  const [parties, setParties] = useState("");
  const [asset, setAsset] = useState("");
  const [network, setNetwork] = useState("");

  let receipt: Receipt | null = null;
  let error = readError;
  let needsIssuer = false;
  if (report !== null && !error) {
    try {
      // Validate the report itself first, so a missing issuer label is a prompt, not a rejection.
      const checked = receiptFromReport(report, { issuer: issuer || "-", parties: parties.split(","), asset, network });
      if (issuer.trim()) receipt = checked;
      else needsIssuer = true;
    } catch (caught) {
      error = caught instanceof ReceiptError ? caught.message : "This report could not be read.";
    }
  }

  const load = (text: string, name: string) => {
    try {
      setReport(JSON.parse(text));
      setReadError(null);
    } catch {
      setReport(null);
      setReadError("That file is not JSON.");
    }
    setSource(name);
  };

  return (
    <div className="grid grid-cols-1 gap-12 lg:grid-cols-[0.9fr_1.1fr] lg:gap-16">
      <div>
        <FileDrop
          id="report-file"
          accept="application/json,.json"
          prompt="Drop audit.json here, or choose the file"
          onText={load}
          onError={(message) => {
            setReport(null);
            setReadError(message);
          }}
        />
        <div className="mt-4 flex flex-wrap items-center justify-between gap-4">
          <span className="mono-xs text-fore-3">{source ? `read: ${source}` : "no report open"}</span>
          <button
            type="button"
            className="link"
            onClick={() => {
              load(JSON.stringify(EXAMPLE_REPORT), "example (synthetic, not a real deal)");
              if (!issuer) setIssuer("0xexample-issuer");
            }}
          >
            Load an example
          </button>
        </div>

        <div className="mt-8">
          <Field id="issuer" label="Issuer" value={issuer} onChange={setIssuer} placeholder="the issuing participant" required />
          <Field id="parties" label="Parties" value={parties} onChange={setParties} placeholder="comma separated, optional" />
          <Field id="asset" label="Asset" value={asset} onChange={setAsset} placeholder="ticker or address, optional" />
          <Field id="network" label="Network" value={network} onChange={setNetwork} placeholder="e.g. eip155:10143, optional" />
          <p className="mono-xs m-0 border-t border-rule pt-4 leading-relaxed text-fore-3">
            The audit report carries the verified flags and the deal commitment only. Issuer, parties,
            asset and network are labels you add; nothing here checks them.
          </p>
        </div>
      </div>

      <div aria-live="polite">
        {error ? (
          <div className="border-t border-rule py-6">
            <p className="label m-0 mb-3">Not a receipt</p>
            <p className="prose m-0">{error}</p>
          </div>
        ) : receipt ? (
          <div>
            <p className="label m-0 mb-4">Private deal receipt</p>
            <Row label="Status">
              <span className={receipt.status === "verified" ? "text-fore" : "text-fore-2"}>{receipt.status}</span>
            </Row>
            <Row label="Deal">
              <span className="leak">{receipt.dealCommitment}</span>
              <span className="leak-tag ml-3">public on chain</span>
            </Row>
            <Row label="Issuer">{receipt.issuer}</Row>
            {receipt.parties.length ? <Row label="Parties">{receipt.parties.join(", ")}</Row> : null}
            {receipt.operationRef ? <Row label="Operation">{receipt.operationRef}</Row> : null}
            <Row label="Agreement">{flagText(receipt.agreementVerified)}</Row>
            <Row label="Payment">
              <span className={receipt.paymentVerified ? "text-fore" : "text-fore-2"}>{flagText(receipt.paymentVerified)}</span>
            </Row>
            <Row label="Delivery">
              <span className="text-fore-2">{DELIVERY_NOTE}</span>
            </Row>
            {receipt.asset ? <Row label="Asset">{receipt.asset}</Row> : null}
            {receipt.network ? <Row label="Network">{receipt.network}</Row> : null}
            {!receipt.paymentVerified ? <p className="prose m-0 border-t border-rule pt-4">{PENDING_NOTE}</p> : null}

            <div className="mt-8 flex flex-wrap items-center gap-6 border-t border-rule pt-6">
              <button
                type="button"
                className="btn"
                onClick={() => download("receipt.html", "text/html", receiptHtml(receipt!))}
              >
                Save as HTML
              </button>
              <button
                type="button"
                className="link"
                onClick={() => download("receipt.json", "application/json", receiptJson(receipt!))}
              >
                Save as JSON
              </button>
            </div>
          </div>
        ) : needsIssuer ? (
          <div className="border-t border-rule py-6">
            <p className="label m-0 mb-3">Report read</p>
            <p className="prose m-0 max-w-[52ch]">Enter the issuing participant to see the receipt.</p>
          </div>
        ) : (
          <div className="border-t border-rule py-6">
            <p className="label m-0 mb-3">Waiting for a report</p>
            <p className="prose m-0 max-w-[52ch]">
              Open the JSON that <code>metropolis_audit.py</code> printed. The receipt appears here as
              soon as it is read and an issuer is entered.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
