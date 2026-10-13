import { BACKENDS } from "@/lib/content";
import { Section } from "./Chrome";
import { Reveal } from "./Reveal";
import { Snippet } from "./Snippet";

/**
 * The two settlement backends, side by side. The protocol on top is the same
 * shape on both — encrypted negotiation, one authorized agreement, shielded
 * settlement, scoped disclosure — so the comparison is about where value moves
 * and how far each deployment has been taken, stated with its limits.
 */
export function Backends() {
  return (
    <Section id="backends" className="pt-24 md:pt-32">
      <Reveal className="section-head">
        <h2 className="display mb-0 max-w-[27ch] text-[clamp(25px,3.5vw,49px)]">
          Two settlement backends.
        </h2>
        <div className="flex flex-col justify-end">
          <p className="prose m-0 max-w-[54ch]">
            The agent-facing surface is the same: negotiate, authorize one agreement, settle, and
            disclose a single deal. Underneath, value moves through a different shielded pool on
            each chain.
          </p>
        </div>
      </Reveal>

      <div className="mt-12 grid grid-cols-1 border-t border-rule lg:grid-cols-2">
        {BACKENDS.map((b, i) => (
          <Reveal
            key={b.chain}
            delay={i * 90}
            className={`flex flex-col gap-6 border-b border-rule py-10 lg:py-12 ${
              i === 0 ? "lg:border-r lg:pr-12" : "lg:pl-12"
            }`}
          >
            <div className="flex flex-wrap items-baseline justify-between gap-4">
              <h3 className="display m-0 text-[clamp(30px,3.6vw,52px)] leading-none">{b.chain}</h3>
              <p className="mono-xs m-0 uppercase tracking-[0.16em] text-fore-3">
                {b.network} · {b.release}
              </p>
            </div>
            <p className="mono-sm m-0 text-fore-2">{b.pool}</p>
            <p className="prose m-0 max-w-[52ch]">{b.summary}</p>

            <ul className="m-0 list-none space-y-2 border-t border-rule p-0 pt-5">
              {b.facts.map((f) => (
                <li key={f} className="mono-xs flex gap-3 leading-relaxed text-fore-2">
                  <span aria-hidden className="text-fore-3">
                    —
                  </span>
                  {f}
                </li>
              ))}
            </ul>

            <Snippet command={b.install} label={`install · ${b.chain.toLowerCase()}`} glow />

            <div className="mt-auto flex flex-col gap-4">
              <p className="mono-xs m-0 max-w-[60ch] leading-relaxed text-fore-3">
                <span className="leak">Limits.</span> {b.limits}
              </p>
              <a href={b.docs} className="link self-start">
                {b.chain} setup ↗
              </a>
            </div>
          </Reveal>
        ))}
      </div>
    </Section>
  );
}
