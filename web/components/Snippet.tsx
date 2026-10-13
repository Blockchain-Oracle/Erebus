"use client";

import { useState } from "react";

/**
 * A command block: one hairline, a mono line, a copy affordance. No fill —
 * structure on this page comes from hairlines, not from cards.
 *
 * The whole command is in the DOM as text, so a no-JS or crawler reader gets it
 * whether or not the copy button ever hydrates.
 */
export function Snippet({
  command,
  label,
  highlight = false,
  accent = false,
  glow = false,
}: {
  command: string;
  label: string;
  /** Ember border and glow, for the one command that shouldn't blend in. */
  highlight?: boolean;
  /** A quiet ember rule on the left edge only, for the one command that
   *  matters most on a reference page — without the full install-card
   *  treatment, which is reserved for the single most load-bearing command
   *  on the whole site. */
  accent?: boolean;
  /** The accent rule with an ember glow spilling off its left edge, for the backend install
   *  commands: louder than `accent`, quieter than the full `highlight` card. */
  glow?: boolean;
}) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  const boxClass = highlight
    ? "install-card"
    : glow
      ? "border border-rule border-l-2 accent-glow"
      : accent
        ? "border border-rule border-l-2"
        : "border border-rule";

  return (
    <div className={boxClass} style={accent || glow ? { borderLeftColor: "var(--color-ember)" } : undefined}>
      <div
        className={`flex items-center justify-between px-4 py-2 ${
          highlight ? "border-b border-ember/40" : "border-b border-rule"
        }`}
      >
        <span className="label" style={highlight ? { color: "var(--color-ember)" } : undefined}>
          {label}
        </span>
        <button
          type="button"
          onClick={copy}
          aria-live="polite"
          className="mono-xs uppercase tracking-[0.16em] text-fore-2 transition-colors hover:text-fore"
        >
          {copied ? "copied" : "copy"}
        </button>
      </div>
      <pre className="m-0 overflow-x-auto px-4 py-4 text-[12px] leading-[1.9] text-fore">
        <code>{command}</code>
      </pre>
    </div>
  );
}
