/**
 * The receipt rules from agents/src/erebus_agents/receipt_explorer.py, for a report read in the
 * browser. This formats a verification an auditor already did; it verifies nothing itself.
 * Keep the two in step: an auditor block is required, a delivery claim is refused, and a missing
 * payment flag is pending, never failure.
 */

export type Receipt = {
  dealCommitment: string;
  issuer: string;
  agreementVerified: boolean;
  paymentVerified: boolean;
  operationRef: string | null;
  parties: string[];
  asset: string | null;
  network: string | null;
  status: "verified" | "agreement_only" | "unverified";
};

export type ReceiptLabels = { issuer: string; parties: string[]; asset: string; network: string };

const COMMITMENT = /^[0-9a-fA-F]{64}$/;

export class ReceiptError extends Error {}

export function receiptFromReport(report: unknown, labels: ReceiptLabels): Receipt {
  if (typeof report !== "object" || report === null || Array.isArray(report)) {
    throw new ReceiptError("This is not a metropolis_audit.py report.");
  }
  const auditor = (report as Record<string, unknown>).auditor;
  if (typeof auditor !== "object" || auditor === null || Array.isArray(auditor)) {
    throw new ReceiptError("Expected a metropolis_audit.py report with an “auditor” block.");
  }
  const flags = auditor as Record<string, unknown>;
  if (flags.delivery_verified === true) {
    throw new ReceiptError("Disclosure never proves delivery, so a report claiming it is refused.");
  }
  const commitment = typeof flags.deal_commitment === "string" ? flags.deal_commitment : "";
  if (!COMMITMENT.test(commitment)) {
    throw new ReceiptError("The report's deal commitment must be 64 hex digits.");
  }
  if (!labels.issuer.trim()) {
    throw new ReceiptError("Enter the issuing participant's identity.");
  }
  const agreementVerified = flags.agreement_verified === true;
  const paymentVerified = flags.payment_verified === true;
  const operation = (report as Record<string, unknown>).operation_ref;
  return {
    dealCommitment: commitment.toLowerCase(),
    issuer: labels.issuer.trim(),
    agreementVerified,
    paymentVerified,
    operationRef: typeof operation === "string" ? operation : null,
    parties: labels.parties.map((party) => party.trim()).filter(Boolean),
    asset: labels.asset.trim() || null,
    network: labels.network.trim() || null,
    status: agreementVerified && paymentVerified ? "verified" : agreementVerified ? "agreement_only" : "unverified",
  };
}

export function receiptJson(receipt: Receipt): string {
  return `${JSON.stringify(
    {
      deal_commitment: receipt.dealCommitment,
      issuer: receipt.issuer,
      parties: receipt.parties,
      agreement_verified: receipt.agreementVerified,
      payment_verified: receipt.paymentVerified,
      delivery_verified: false,
      asset: receipt.asset,
      network: receipt.network,
      operation_ref: receipt.operationRef,
      status: receipt.status,
    },
    null,
    2,
  )}\n`;
}

export const PENDING_NOTE = "Payment pending is not evidence of non-payment; re-check later, do not pay again.";
export const DELIVERY_NOTE = "not proven (disclosure never proves delivery)";

const escape = (value: string) =>
  value.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** The same self-contained page `receipt_explorer.py --format html` writes. */
export function receiptHtml(receipt: Receipt): string {
  const row = (label: string, value: string, tone = "") =>
    `<tr><th>${escape(label)}</th><td${tone ? ` class="${tone}"` : ""}>${escape(value)}</td></tr>`;
  const flag = (label: string, value: boolean) => row(label, value ? "verified" : "pending", value ? "ok" : "warn");
  const body =
    row("Status", receipt.status, receipt.status === "verified" ? "ok" : "warn") +
    row("Deal", receipt.dealCommitment) +
    row("Issuer", receipt.issuer) +
    (receipt.parties.length ? row("Parties", receipt.parties.join(", ")) : "") +
    (receipt.operationRef ? row("Operation", receipt.operationRef) : "") +
    flag("Agreement", receipt.agreementVerified) +
    flag("Payment", receipt.paymentVerified) +
    row("Delivery", DELIVERY_NOTE, "muted") +
    (receipt.asset ? row("Asset", receipt.asset) : "") +
    (receipt.network ? row("Network", receipt.network) : "");
  const note = receipt.paymentVerified ? "" : `<p class="note">${escape(PENDING_NOTE)}</p>`;
  return (
    '<!doctype html>\n<html lang="en"><head><meta charset="utf-8">' +
    '<meta name="viewport" content="width=device-width, initial-scale=1">' +
    "<title>Private deal receipt</title><style>" +
    "body{font:15px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;background:#0a0908;color:#e8e3dc;" +
    "margin:0;padding:3rem 1.25rem}" +
    "main{max-width:44rem;margin:0 auto}" +
    "h1{font-size:1.1rem;letter-spacing:.02em;margin:0 0 1.5rem}" +
    "table{width:100%;border-collapse:collapse}" +
    "th,td{text-align:left;padding:.5rem .25rem;border-bottom:1px solid #26221d;vertical-align:top;" +
    "overflow-wrap:anywhere}" +
    "th{width:11rem;color:#9a938a;font-weight:400}" +
    ".ok{color:#7bd88f}.warn{color:#f0b429}.muted{color:#9a938a}" +
    ".note{margin-top:1.5rem;color:#9a938a;font-size:13px}" +
    "</style></head><body><main><h1>Private deal receipt</h1><table>" +
    body +
    "</table>" +
    note +
    "</main></body></html>\n"
  );
}

/** Clearly synthetic, so a first-time visitor can see the tool work. Not a real deal. */
export const EXAMPLE_REPORT = {
  operation_ref: "1".repeat(64),
  auditor: {
    agreement_verified: true,
    payment_verified: true,
    delivery_verified: false,
    deal_commitment: "ab".repeat(32),
  },
};
