import { useId, useState } from "react";
import type { ComponentPropsWithRef } from "react";

import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";

export type CitationReference = {
  number: number;
  label: string;
  host: string;
  url: string;
  description?: string;
  image?: string;
};

function sourceHref(url: string) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.href : undefined;
  } catch {
    return undefined;
  }
}

const markerClass =
  "mx-0.5 inline-flex min-w-4 items-center justify-center rounded-pill bg-surface-muted px-1 text-xs leading-4 font-medium text-text-muted";

/** Citation markers retain unknown references as text and expose sources on hover and focus. */
export function InlineCitations({
  text,
  references,
  sourcesLabel,
  className,
  ...props
}: ComponentPropsWithRef<"section"> & {
  text: string;
  references: readonly CitationReference[];
  sourcesLabel: string;
}) {
  const id = useId();
  const [dismissed, setDismissed] = useState<number>();
  return (
    <section
      {...props}
      className={cn("min-w-0 text-content leading-content break-words", className)}
    >
      <p>
        {[...text.matchAll(/\[\d+\]|(?:(?!\[\d+\])[\s\S])+/gu)].map((token) => {
          const key = token.index;
          const [part] = token;
          const match = /^\[(?<number>\d+)\]$/u.exec(part);
          const reference = match
            ? references.find((item) => item.number === Number(match.groups?.number))
            : undefined;
          const href = reference ? sourceHref(reference.url) : undefined;
          if (reference === undefined || href === undefined) {
            return <span key={key}>{part}</span>;
          }
          const tooltipId = `${id}-${key}`;
          return (
            <span key={key} className="group relative inline [anchor-scope:--citation]">
              <a
                href={href}
                target="_blank"
                rel="noreferrer"
                onFocus={() => {
                  setDismissed(undefined);
                }}
                onMouseEnter={() => {
                  setDismissed(undefined);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    setDismissed(key);
                  }
                }}
                aria-describedby={tooltipId}
                aria-label={`${reference.number}: ${reference.label}`}
                className={cn(
                  markerClass,
                  "align-super transition-colors duration-150 [anchor-name:--citation] hover:bg-surface-hover hover:text-ink focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none",
                )}
              >
                {reference.number}
              </a>
              <span
                hidden={dismissed === key}
                id={tooltipId}
                role="tooltip"
                aria-label={reference.label}
                className="invisible absolute bottom-full left-1/2 z-20 mb-1.5 w-max max-w-[calc(100vw-24px)] -translate-x-1/2 scale-97 rounded-control bg-inverse/90 px-2 py-1 text-xs leading-4 text-inverse-text opacity-0 backdrop-blur-sm transition duration-200 ease-assistant-citation group-focus-within:visible group-focus-within:scale-100 group-focus-within:opacity-100 group-hover:visible group-hover:scale-100 group-hover:opacity-100 supports-[position-anchor:--citation]:fixed supports-[position-anchor:--citation]:inset-auto supports-[position-anchor:--citation]:translate-x-0 supports-[position-anchor:--citation]:[position-anchor:--citation] supports-[position-anchor:--citation]:[position-area:top] supports-[position-anchor:--citation]:[position-try-fallbacks:flip-block] motion-reduce:transition-none"
              >
                <a
                  href={href}
                  target="_blank"
                  rel="noreferrer"
                  className="block max-w-64 space-y-1 whitespace-normal focus-visible:outline-2 focus-visible:outline-primary"
                >
                  {reference.image !== undefined && sourceHref(reference.image) !== undefined ? (
                    <img
                      src={reference.image}
                      alt=""
                      className="h-24 w-full rounded-control object-cover"
                    />
                  ) : null}
                  <span className="block font-medium">{reference.label}</span>
                  <span className="block text-inverse-muted">{reference.host}</span>
                  {reference.description === undefined ? null : (
                    <span className="block">{reference.description}</span>
                  )}
                </a>
              </span>
            </span>
          );
        })}
      </p>
      <nav aria-label={sourcesLabel} className="mt-3 flex flex-col gap-1.5 pt-2">
        {references.map((reference) => {
          const href = sourceHref(reference.url);
          if (href === undefined) {
            return null;
          }
          return (
            <a
              key={reference.number}
              href={href}
              target="_blank"
              rel="noreferrer"
              className="group flex min-w-0 items-center gap-1.5 rounded-pill text-xs leading-5 text-text-muted focus-visible:outline-2 focus-visible:outline-primary"
            >
              <span className={markerClass}>{reference.number}</span>
              <span className="min-w-0 truncate text-ink">{reference.label}</span>
              <span aria-hidden="true">·</span>
              <span className="shrink-0 transition-colors duration-160 group-hover:text-ink group-focus-visible:text-ink motion-reduce:transition-none">
                {reference.host}
              </span>
              <Icon
                name="arrow"
                className="transition-citation-arrow size-3 shrink-0 translate-y-0.5 -rotate-45 opacity-0 group-hover:scale-100 group-hover:opacity-100 group-focus-visible:translate-y-0 group-focus-visible:opacity-100 motion-reduce:transition-none"
              />
            </a>
          );
        })}
      </nav>
    </section>
  );
}
