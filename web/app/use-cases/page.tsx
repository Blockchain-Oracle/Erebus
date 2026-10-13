import type { Metadata } from "next";
import { Header, Section } from "@/components/Chrome";
import { Footer } from "@/components/Footer";
import { Reveal } from "@/components/Reveal";
import { Snippet } from "@/components/Snippet";
import { LOOP, NOT_YET, USE_CASES, metropolisDoc } from "@/lib/usecases";

export const metadata: Metadata = {
  title: "Erebus · Use cases",
  description:
    "What agents build on Erebus: discovery, purchases, pay-per-call APIs, prepaid metering and deal receipts, each running on the participants' own servers.",
};

function SourceLinks({ paths }: { paths: string[] }) {
  return (
    <ul className="m-0 flex list-none flex-wrap gap-x-6 gap-y-2 p-0">
      {paths.map((path) => (
        <li key={path}>
          <a
            href={metropolisDoc(path)}
            className="mono-xs break-all text-fore-3 underline decoration-transparent underline-offset-[5px] transition hover:text-fore hover:decoration-rule-2"
          >
            {path} ↗
          </a>
        </li>
      ))}
    </ul>
  );
}

export default function UseCases() {
  return (
    <>
      <Header />
      <main>
        <Section className="pt-28 md:pt-36">
          <div className="max-w-[64ch]">
            <p className="label m-0 mb-6">Use cases · Monad backend</p>
            <h1 className="display mb-0 text-[clamp(36px,6vw,80px)]">Built on Erebus.</h1>
            <p className="lead mt-8">
              These run on the Monad backend, on testnet, through the participants&rsquo; own Erebus
              servers. An agent supplies an operation ID and sequences the calls. Price, keys and
              settlement mode stay in each operator&rsquo;s files.
            </p>
            <p className="mt-6 flex flex-wrap gap-x-7 gap-y-3">
              <a href="https://docs.erebusagents.live/monad" className="link">
                Monad setup ↗
              </a>
              <a href="https://docs.erebusagents.live/monad/use-cases" className="link">
                Use cases in the docs ↗
              </a>
            </p>
          </div>

          <nav aria-label="Use cases" className="mt-12 border-t border-rule pt-6">
            <ul className="m-0 flex list-none flex-wrap gap-x-7 gap-y-3 p-0">
              {[{ slug: "loop", name: "The loop" }, ...USE_CASES, { slug: "tools", name: "Tools" }].map((item) => (
                <li key={item.slug}>
                  <a href={`#${item.slug}`} className="link">
                    {item.name}
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        </Section>

        <Section id="loop" className="pt-20 md:pt-28">
          <Reveal className="section-head">
            <h2 className="display mb-0 text-[clamp(25px,3.5vw,49px)]">The loop under all of them.</h2>
            <div className="flex flex-col justify-end">
              <p className="prose m-0 max-w-[54ch]">
                Every use case below is a layer over the same sequence. Each participant runs its
                own MCP server over native Rust; the buyer settles at most once and resolves any
                doubt by observation.
              </p>
            </div>
          </Reveal>

          <ol className="m-0 mt-12 list-none p-0">
            {LOOP.map((step, i) => (
              <Reveal
                as="li"
                key={step.id}
                delay={i * 60}
                className="group grid grid-cols-1 gap-3 border-t border-rule py-7 transition-colors duration-300 hover:border-rule-2 md:grid-cols-[3rem_16rem_1fr] md:gap-10"
              >
                <span className="mono-xs pt-1 uppercase tracking-[0.16em] text-fore-3">{step.id}</span>
                <div>
                  <p className="m-0 text-[15px] text-fore">{step.tool}</p>
                  <p className="mono-xs m-0 mt-2 uppercase tracking-[0.14em] text-fore-3">{step.who}</p>
                </div>
                <div>
                  <p className="prose m-0 max-w-[60ch]">{step.body}</p>
                  <div className="mt-3">
                    <SourceLinks paths={[step.source]} />
                  </div>
                </div>
              </Reveal>
            ))}
          </ol>
        </Section>

        {USE_CASES.map((useCase) => (
          <Section key={useCase.slug} id={useCase.slug} className="pt-20 md:pt-28">
            <Reveal className="section-head">
              <h2 className="display mb-0 text-[clamp(25px,3.5vw,49px)]">{useCase.name}.</h2>
              <div className="flex flex-col justify-end">
                <p className="prose m-0 max-w-[54ch]">{useCase.line}</p>
              </div>
            </Reveal>

            <div className="mt-10 grid grid-cols-1 gap-10 lg:grid-cols-[1.1fr_0.9fr] lg:gap-16">
              <div>
                <p className="label m-0 mb-4">How it works</p>
                <ol className="m-0 list-none p-0">
                  {useCase.how.map((line, i) => (
                    <li key={line} className="grid grid-cols-[2rem_1fr] gap-3 border-t border-rule py-4">
                      <span className="mono-xs pt-1 text-fore-3">{String(i + 1).padStart(2, "0")}</span>
                      <p className="prose m-0">{line}</p>
                    </li>
                  ))}
                </ol>
              </div>

              <div>
                <p className="label m-0 mb-4">What a chain reader sees</p>
                {useCase.public.length ? (
                  <ul className="m-0 list-none p-0">
                    {useCase.public.map((fact) => (
                      <li key={fact} className="border-t border-rule py-4">
                        <span className="leak-tag">public</span>
                        <p className="prose m-0 mt-2">{fact}</p>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="prose m-0 border-t border-rule py-4">
                    Nothing on chain. The index is local, and a descriptor is a signed advertisement
                    its seller publishes on purpose.
                  </p>
                )}
                {useCase.tool ? (
                  <div className="mt-8">
                    <a href={useCase.tool.href} className="link">
                      {useCase.tool.label} ↗
                    </a>
                  </div>
                ) : null}
              </div>
            </div>

            <div className="mt-10">
              <Snippet label="run it" command={useCase.command} />
              <div className="mt-4">
                <SourceLinks paths={useCase.sources} />
              </div>
            </div>
          </Section>
        ))}

        <Section id="tools" className="pt-20 md:pt-28">
          <Reveal className="section-head">
            <h2 className="display mb-0 text-[clamp(25px,3.5vw,49px)]">Tools in the browser.</h2>
            <div className="flex flex-col justify-end">
              <p className="prose m-0 max-w-[54ch]">
                Two pages that read files you already have. They run entirely in this tab: nothing is
                uploaded, and neither asks for a key.
              </p>
            </div>
          </Reveal>
          <ul className="m-0 mt-10 list-none p-0">
            {[
              { href: "/tools/receipt/", name: "Receipt viewer", body: "Open a metropolis_audit.py report and read it as a receipt, then save it as HTML or JSON." },
              { href: "/tools/discovery/", name: "Discovery browser", body: "Open a discovery index, filter sellers, and check a descriptor file against the digest its record pinned." },
            ].map((tool) => (
              <li key={tool.href} className="border-t border-rule">
                <a href={tool.href} className="group grid grid-cols-1 gap-3 py-7 md:grid-cols-[16rem_1fr_auto] md:gap-10">
                  <span className="display text-[clamp(19px,2.2vw,31px)] leading-[1.06] transition-transform duration-300 group-hover:translate-x-1.5">
                    {tool.name}
                  </span>
                  <span className="prose max-w-[56ch]">{tool.body}</span>
                  <span className="link self-center">Open ↗</span>
                </a>
              </li>
            ))}
          </ul>
        </Section>

        <Section className="pt-20 md:pt-28">
          <Reveal className="section-head">
            <h2 className="display mb-0 text-[clamp(25px,3.5vw,49px)]">Not yet on the real path.</h2>
            <div className="flex flex-col justify-end">
              <p className="prose m-0 max-w-[54ch]">
                On the real path, price, asset and counterparty are fixed by operator files and
                enforced in Rust, and negotiation returns no amount. These need an interface change
                before they can run there.
              </p>
            </div>
          </Reveal>
          <ul className="m-0 mt-10 list-none p-0">
            {NOT_YET.map((item) => (
              <li key={item.name} className="grid grid-cols-1 gap-3 border-t border-rule py-6 md:grid-cols-[16rem_1fr] md:gap-10">
                <span className="text-[15px] text-fore">{item.name}</span>
                <p className="prose m-0 max-w-[60ch]">{item.why}</p>
              </li>
            ))}
          </ul>
          <p className="mono-xs mt-6 max-w-[74ch] border-t border-rule pt-6 leading-relaxed text-fore-3">
            Unaudited. No external security review has been done. Do not put real value through it.
          </p>
        </Section>
      </main>
      <Footer />
    </>
  );
}
