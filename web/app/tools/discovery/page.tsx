import type { Metadata } from "next";
import { Header, Section } from "@/components/Chrome";
import { DiscoveryTool } from "@/components/DiscoveryTool";
import { Footer } from "@/components/Footer";
import { metropolisDoc } from "@/lib/usecases";

export const metadata: Metadata = {
  title: "Erebus · Discovery browser",
  description: "Browse a discovery index of sellers and check a descriptor file against its pinned digest. Runs in your browser; nothing is uploaded.",
};

export default function DiscoveryPage() {
  return (
    <>
      <Header />
      <main>
        <Section className="pt-28 md:pt-36">
          <div className="max-w-[64ch]">
            <p className="label m-0 mb-6">
              <a href="/use-cases/#discovery" className="hover:text-fore">Use cases</a> / Discovery browser
            </p>
            <h1 className="display mb-0 text-[clamp(36px,6vw,80px)]">Find a counterparty.</h1>
            <p className="lead mt-8">
              Open the index <code className="mono-sm text-fore">discovery_demo.py</code> writes and search
              sellers by capability, asset, price and settlement mode. A record points to a seller&rsquo;s signed
              descriptor; it is not proof of anything.
            </p>
          </div>
        </Section>

        <Section className="pt-14 md:pt-20">
          <div className="section-head mb-10">
            <p className="prose m-0 max-w-[60ch]">
              The price band is the seller&rsquo;s unsigned claim and only chooses whom to approach. The agreed
              price is fixed by the operators&rsquo; files and capped by the buyer&rsquo;s maximum price.
            </p>
            <div className="flex flex-col justify-end">
              <a href={metropolisDoc("agents/src/erebus_agents/discovery.py")} className="link self-start">
                The index, as a CLI ↗
              </a>
            </div>
          </div>
          <DiscoveryTool />
        </Section>
      </main>
      <Footer />
    </>
  );
}
