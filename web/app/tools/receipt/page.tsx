import type { Metadata } from "next";
import { Header, Section } from "@/components/Chrome";
import { Footer } from "@/components/Footer";
import { ReceiptTool } from "@/components/ReceiptTool";
import { metropolisDoc } from "@/lib/usecases";

export const metadata: Metadata = {
  title: "Erebus · Receipt viewer",
  description: "Read a metropolis_audit.py report as a deal receipt. Runs in your browser; nothing is uploaded.",
};

export default function ReceiptPage() {
  return (
    <>
      <Header />
      <main>
        <Section className="pt-28 md:pt-36">
          <div className="max-w-[64ch]">
            <p className="label m-0 mb-6">
              <a href="/use-cases/#receipt" className="hover:text-fore">Use cases</a> / Receipt viewer
            </p>
            <h1 className="display mb-0 text-[clamp(36px,6vw,80px)]">Read a deal receipt.</h1>
            <p className="lead mt-8">
              Open the report an independent auditor produced with <code className="mono-sm text-fore">metropolis_audit.py</code>.
              This page formats that verification; it does not verify anything itself.
            </p>
          </div>
        </Section>

        <Section className="pt-14 md:pt-20">
          <div className="section-head mb-10">
            <p className="prose m-0 max-w-[60ch]">
              Three facts stay apart: the agreement matched the disclosed deal, the payment matched the
              agreement, and delivery, which disclosure never proves. A pending payment is pending, not
              failed.
            </p>
            <div className="flex flex-col justify-end">
              <a href={metropolisDoc("agents/src/erebus_agents/receipt_explorer.py")} className="link self-start">
                The same rules, as a CLI ↗
              </a>
            </div>
          </div>
          <ReceiptTool />
        </Section>
      </main>
      <Footer />
    </>
  );
}
