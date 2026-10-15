import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { cn } from "../../classes";
import { useImageDotMotion } from "./knowledge-motion";
import { AssistantDisclosure, AssistantPanel, AssistantProgress, PanelHeading } from "./shared";
import { ThinkingState } from "./thinking-state";
import { AssistantModal } from "./thread";

export type SourceReference = {
  id: string;
  title: string;
  href: string;
  kind?: "url" | "document";
  excerpt?: string;
  domain?: string;
  detail?: string;
  author?: string;
  publishedAt?: string;
};

function sourceAddress(source: SourceReference) {
  if (source.kind === "document") {
    return { href: undefined, domain: source.domain };
  }
  try {
    const url = new URL(source.href);
    return {
      href: url.protocol === "https:" || url.protocol === "http:" ? url.href : undefined,
      domain: source.domain ?? (url.hostname.length > 0 ? url.hostname : undefined),
    };
  } catch {
    return { href: undefined, domain: source.domain };
  }
}

function publishedDate(value: string | undefined, locale: string) {
  if (value === undefined) {
    return undefined;
  }
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) {
    return undefined;
  }
  const options: Intl.DateTimeFormatOptions = { month: "short", year: "numeric" };
  try {
    return new Intl.DateTimeFormat(locale, options).format(date);
  } catch {
    return new Intl.DateTimeFormat("en-US", options).format(date);
  }
}

/** Source disclosure previews domains before exposing keyboard-accessible reference cards. */
export function Sources({
  sources,
  label,
  open,
  onOpenChange,
  locale = "en-US",
}: {
  sources: readonly SourceReference[];
  label: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  locale?: string;
}) {
  const domains = sources
    .flatMap((source) => {
      const { domain } = sourceAddress(source);
      return domain === undefined ? [] : [{ id: source.id, domain }];
    })
    .slice(0, 3);
  return (
    <details
      open={open}
      onToggle={(event) => {
        if (open === undefined) {
          onOpenChange?.(event.currentTarget.open);
        }
      }}
      className="assistant-disclosure group min-w-0 text-ui text-ink"
      data-motion="tool"
    >
      <summary
        onClick={(event) => {
          if (open !== undefined) {
            event.preventDefault();
            onOpenChange?.(!open);
          }
        }}
        className="inline-flex min-h-control cursor-pointer list-none items-center gap-2 rounded-pill bg-surface-muted px-3 py-2 text-ui leading-ui font-medium text-text-muted transition-colors duration-150 hover:bg-surface-hover hover:text-ink focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none pointer-coarse:min-h-control-touch"
      >
        {domains.length > 0 ? (
          <span aria-hidden="true" className="flex -space-x-1">
            {domains.map(({ id, domain }) => (
              <span
                key={id}
                className="flex size-4 items-center justify-center rounded-pill bg-surface-hover text-xs ring-1 ring-surface"
              >
                {domain.charAt(0).toUpperCase()}
              </span>
            ))}
          </span>
        ) : null}
        <span>{label}</span>
        <span className="font-mono text-xs tabular-nums">{sources.length}</span>
        <Icon
          name="chevron"
          className="size-3 transition-transform duration-200 ease-swarmie group-open:rotate-180 motion-reduce:transition-none"
        />
      </summary>
      <nav aria-label={label} className="pt-2.5">
        <ul className="grid grid-cols-2 gap-2">
          {sources.map((source) => {
            const { href, domain } = sourceAddress(source);
            const meta = [source.author, publishedDate(source.publishedAt, locale), source.detail]
              .filter(Boolean)
              .join(" · ");
            const content = (
              <>
                {domain === undefined ? null : (
                  <span className="flex min-w-0 items-center gap-1.5 text-xs text-text-muted">
                    <span
                      aria-hidden="true"
                      className="flex size-4 shrink-0 items-center justify-center rounded-pill bg-surface-muted font-medium"
                    >
                      {domain.charAt(0).toUpperCase()}
                    </span>
                    <span className="truncate font-mono">{domain}</span>
                  </span>
                )}
                <span className="line-clamp-2 text-ui leading-ui font-medium text-ink">
                  {source.title}
                </span>
                {source.excerpt === undefined ? null : (
                  <span className="line-clamp-2 text-xs leading-relaxed text-text-muted">
                    {source.excerpt}
                  </span>
                )}
                {meta.length === 0 ? null : (
                  <span className="truncate text-xs text-text-muted">{meta}</span>
                )}
              </>
            );
            const cardClass =
              "flex min-w-0 flex-col gap-1.5 rounded-panel bg-surface p-3 transition-transform duration-150 hover:-translate-y-px focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none";
            return (
              <li key={source.id} className="min-w-0 motion-safe:animate-assistant-tool-step">
                {href === undefined ? (
                  <div className={cardClass}>{content}</div>
                ) : (
                  <a href={href} target="_blank" rel="noreferrer" className={cardClass}>
                    {content}
                  </a>
                )}
              </li>
            );
          })}
        </ul>
      </nav>
    </details>
  );
}

export function WebSearch({
  query,
  sources,
  label,
  running = false,
  statusLabel,
}: {
  query: string;
  sources: readonly SourceReference[];
  label: string;
  running?: boolean;
  statusLabel?: string;
}) {
  return (
    <AssistantPanel aria-busy={running}>
      <PanelHeading>
        <span className="flex items-center gap-2">
          <Icon name="search" />
          {running ? <ThinkingState>{query}</ThinkingState> : query}
        </span>
      </PanelHeading>
      <output className="block text-xs text-text-muted">{statusLabel}</output>
      <Sources sources={sources} label={label} />
    </AssistantPanel>
  );
}

/** Image failures are scoped to the URL, so a new result can load after a failed one. */
function KnowledgeImage({
  src,
  alt,
  className,
  state = "loading",
  statusLabel,
}: {
  src: string;
  alt: string;
  className?: string | undefined;
  state?: "generating" | "filtered" | "loading" | "loaded" | "error" | undefined;
  statusLabel?: string | undefined;
}) {
  const [loadedSource, setLoadedSource] = useState<string>();
  const [failedSource, setFailedSource] = useState<string>();
  if (state === "generating" || state === "filtered" || state === "error" || failedSource === src) {
    return (
      <span
        className={cn(
          "flex aspect-square items-center justify-center rounded-panel bg-surface-muted text-text-muted",
          className,
        )}
      >
        <Icon
          name={state === "generating" ? "loading" : "file"}
          className={state === "generating" ? "motion-safe:animate-assistant-spin" : undefined}
        />
        <span>{statusLabel ?? alt}</span>
      </span>
    );
  }
  return (
    <img
      src={src}
      alt={alt}
      onLoad={() => {
        setLoadedSource(src);
      }}
      onError={() => {
        setFailedSource(src);
      }}
      className={cn(
        "transition-opacity duration-300 motion-reduce:transition-none",
        loadedSource === src || state === "loaded"
          ? "opacity-100"
          : "opacity-50 motion-safe:animate-pulse",
        className,
      )}
    />
  );
}

export function LinkPreview({
  source,
  image,
  imageSrc,
  favicon,
  compact = false,
}: {
  source: SourceReference;
  image?: ReactNode;
  imageSrc?: string;
  favicon?: string;
  compact?: boolean;
}) {
  const { href } = sourceAddress(source);
  const content = (
    <>
      {imageSrc === undefined ? (
        image
      ) : (
        <KnowledgeImage
          src={imageSrc}
          alt=""
          className={compact ? "size-16 object-cover" : "aspect-video w-full object-cover"}
        />
      )}
      <span className="flex min-w-0 flex-col gap-2">
        {favicon === undefined ? null : <KnowledgeImage src={favicon} alt="" className="size-4" />}
        <span className="text-xs text-text-muted">{source.domain}</span>
        <span className="font-medium">{source.title}</span>
        {source.excerpt === undefined ? null : (
          <span className="text-content leading-content text-ink">{source.excerpt}</span>
        )}
      </span>
    </>
  );
  const classes = cn(
    "flex gap-3 rounded-panel bg-surface p-3 transition-transform duration-150 ease-assistant-label hover:-translate-y-px focus-visible:outline-2 focus-visible:outline-primary motion-reduce:transition-none",
    compact ? "items-center" : "flex-col",
  );
  return href === undefined ? (
    <div className={classes}>{content}</div>
  ) : (
    <a href={href} className={classes}>
      {content}
    </a>
  );
}

export function ImageGeneration({
  label,
  progress,
  children,
  onRegenerate,
  regenerateLabel,
}: {
  label: string;
  progress?: number;
  children?: ReactNode;
  onRegenerate?: () => void;
  regenerateLabel?: string;
}) {
  const dots = useImageDotMotion(children === undefined || children === null);
  return (
    <AssistantPanel aria-busy={children === undefined || children === null}>
      <PanelHeading>{label}</PanelHeading>
      {children === undefined || children === null ? (
        <div className="flex aspect-video items-center justify-center rounded-control bg-surface-muted">
          <span ref={dots} aria-hidden="true" className="grid grid-cols-8 gap-3">
            {Array.from({ length: 64 }, (_, dot) => (
              <span
                key={`dot-${dot}`}
                className="size-1 rounded-full bg-primary/30 transition-opacity duration-500 motion-reduce:transition-none"
              />
            ))}
          </span>
        </div>
      ) : (
        <div className="motion-safe:animate-assistant-image-reveal">{children}</div>
      )}
      {onRegenerate === undefined ? null : (
        <Button variant="ghost" onClick={onRegenerate}>
          {regenerateLabel}
        </Button>
      )}
      {progress === undefined ? null : <AssistantProgress label={label} value={progress} />}
    </AssistantPanel>
  );
}

export type GalleryImage = {
  id: string;
  src: string;
  alt: string;
  caption?: string;
  state?: "generating" | "filtered" | "loading" | "loaded" | "error";
  statusLabel?: string;
};

export function ImageGallery({
  images,
  onSelect,
  label,
  previousLabel = "Previous image",
  nextLabel = "Next image",
  closeLabel = "Close image",
}: {
  images: readonly GalleryImage[];
  onSelect?: (image: GalleryImage) => void;
  label: string;
  previousLabel?: string;
  nextLabel?: string;
  closeLabel?: string;
}) {
  const [activeId, setActiveId] = useState<string>();
  const index = images.findIndex((image) => image.id === activeId);
  const active = images[index];
  const select = (image: GalleryImage) => {
    setActiveId(image.id);
    onSelect?.(image);
  };
  useEffect(() => {
    if (active === undefined) {
      return undefined;
    }
    const handleKey = (event: KeyboardEvent) => {
      let image: GalleryImage | undefined;
      if (event.key === "ArrowLeft") {
        image = images[index - 1];
      }
      if (event.key === "ArrowRight") {
        image = images[index + 1];
      }
      if (image === undefined) {
        return;
      }
      event.preventDefault();
      setActiveId(image.id);
      onSelect?.(image);
    };
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("keydown", handleKey);
    };
  }, [active, images, index, onSelect]);

  return (
    <div className="@container">
      <ul aria-label={label} className="grid grid-cols-2 gap-3 @lg:grid-cols-3">
        {images.map((image) => (
          <li key={image.id}>
            <Button
              variant="ghost"
              className="h-auto w-full flex-col rounded-panel p-0 whitespace-normal transition-transform duration-150 hover:scale-101 motion-reduce:transition-none"
              onClick={() => {
                select(image);
              }}
              aria-label={image.caption ?? image.alt}
            >
              <KnowledgeImage
                {...image}
                className="aspect-square w-full rounded-panel object-cover"
              />
              <span>{image.caption ?? image.alt}</span>
            </Button>
          </li>
        ))}
      </ul>
      <AssistantModal
        open={active !== undefined}
        onOpenChange={(open) => {
          if (!open) {
            setActiveId(undefined);
          }
        }}
        title={active?.caption ?? active?.alt ?? label}
        closeLabel={closeLabel}
      >
        {active === undefined ? null : (
          <div>
            <KnowledgeImage {...active} className="max-h-[70dvh] w-full object-contain" />
            <div className="mt-3 flex justify-between gap-2">
              <Button
                variant="ghost"
                disabled={index <= 0}
                onClick={() => {
                  const image = images[index - 1];
                  if (image) {
                    select(image);
                  }
                }}
              >
                {previousLabel}
              </Button>
              <output>
                {index + 1}/{images.length}
              </output>
              <Button
                variant="ghost"
                disabled={index >= images.length - 1}
                onClick={() => {
                  const image = images[index + 1];
                  if (image) {
                    select(image);
                  }
                }}
              >
                {nextLabel}
              </Button>
            </div>
          </div>
        )}
      </AssistantModal>
    </div>
  );
}

export type RetrievalChunk = {
  id: string;
  title: string;
  text: string;
  relevance: string;
  score?: number;
  href: string;
};

export function RetrievalChunks({
  chunks,
  label,
  running = false,
  statusLabel,
}: {
  chunks: readonly RetrievalChunk[];
  running?: boolean;
  statusLabel?: string;
  label: string;
}) {
  return (
    <div aria-label={label} aria-busy={running} className="space-y-3">
      <output>
        {running ? <ThinkingState>{statusLabel ?? label}</ThinkingState> : statusLabel}
      </output>
      {chunks.map((chunk) => (
        <AssistantDisclosure
          key={chunk.id}
          className="motion-safe:animate-assistant-tool-step"
          summary={
            <span className="flex flex-wrap items-center justify-between gap-2">
              {chunk.title}
              <Badge>{chunk.relevance}</Badge>
            </span>
          }
        >
          {chunk.score === undefined ? null : (
            <AssistantProgress label={chunk.relevance} value={chunk.score} max={1} />
          )}
          <blockquote className="text-content leading-content">{chunk.text}</blockquote>
          <a href={chunk.href} className="mt-3 inline-block text-ui text-primary underline">
            {chunk.title}
          </a>
        </AssistantDisclosure>
      ))}
    </div>
  );
}

export function DocumentReference({
  source,
  page,
  quote,
  anchors,
  activePage,
  onJump,
}: {
  source: SourceReference;
  page: string;
  quote: string;
  anchors?: readonly { page: number; label: string; quote: string }[];
  activePage?: number;
  onJump?: (page: number) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>
        <a href={source.href} className="flex items-center gap-2 text-primary underline">
          <Icon name="file" />
          {source.title}
        </a>
      </PanelHeading>
      <blockquote className="text-content leading-content">{quote}</blockquote>
      <p className="mt-3 text-xs text-text-muted">{page}</p>
      <ul className="space-y-2">
        {anchors?.map((anchor) => (
          <li key={`${anchor.page}-${anchor.quote}`}>
            <Button
              variant="navigation"
              selected={anchor.page === activePage}
              onClick={() => {
                onJump?.(anchor.page);
              }}
              className="h-auto w-full flex-col items-start whitespace-normal"
            >
              <span>{anchor.label}</span>
              <span className="text-xs">{anchor.quote}</span>
            </Button>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export type MemoryEntry = {
  id: string;
  text: string;
  origin: string;
  change?: "added" | "updated" | "existing";
  changeLabel?: string;
};

export function MemoryChips({
  entries,
  label,
  removeLabel,
  onRemove,
}: {
  entries: readonly MemoryEntry[];
  label: string;
  removeLabel: string;
  onRemove: (id: string) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>
        <span className="flex items-center gap-2">
          <Icon name="memory" />
          {label}
        </span>
      </PanelHeading>
      <ul className="space-y-3">
        {entries.map((entry) => (
          <li
            key={entry.id}
            className="flex items-start gap-3 motion-safe:animate-assistant-node-in"
          >
            <div className="min-w-0 flex-1">
              <p className="text-ui">{entry.text}</p>
              {entry.change === undefined ? null : (
                <Badge tone={entry.change === "existing" ? "neutral" : "agent"}>
                  {entry.changeLabel ?? entry.change}
                </Badge>
              )}
              <p className="text-xs text-text-muted">{entry.origin}</p>
            </div>
            <Button
              variant="ghost"
              onClick={() => {
                onRemove(entry.id);
              }}
              aria-label={`${removeLabel}: ${entry.text}`}
            >
              <Icon name="close" />
              {removeLabel}
            </Button>
          </li>
        ))}
      </ul>
    </AssistantPanel>
  );
}

export type ResearchSection = {
  id: string;
  title: string;
  content: ReactNode;
  sources: readonly SourceReference[];
  state: "pending" | "writing" | "done";
};

const researchIcons = { pending: "clock", writing: "loading", done: "check" } as const;

export function ResearchReport({
  title,
  sections,
  sourcesLabel,
}: {
  title: string;
  sections: readonly ResearchSection[];
  sourcesLabel: string;
}) {
  return (
    <article className="space-y-4 text-content leading-content">
      <h2 className="text-document">{title}</h2>
      <output className="text-xs text-text-muted">
        {sections.filter((section) => section.state === "done").length}/{sections.length}
      </output>
      <nav aria-label={title} className="flex flex-wrap gap-3">
        {sections.map((section) => (
          <a key={section.id} href={`#${section.id}`} className="text-ui text-primary underline">
            {section.title}
          </a>
        ))}
      </nav>
      {sections.map((section) => (
        <section
          key={section.id}
          id={section.id}
          aria-busy={section.state === "writing"}
          className="space-y-3 motion-safe:animate-assistant-receipt"
        >
          <h3 className="flex items-center gap-2 text-title">
            <Icon
              name={researchIcons[section.state]}
              className={
                section.state === "writing" ? "motion-safe:animate-assistant-spin" : undefined
              }
            />
            {section.title}
          </h3>
          {section.content}
          <Sources label={sourcesLabel} sources={section.sources} />
        </section>
      ))}
    </article>
  );
}

export type MapPlace = {
  id: string;
  name: string;
  description: string;
  latitude: number;
  longitude: number;
  href?: string;
};

/** Map providers render into the map slot. Every location also has an accessible list. */
export function MapAnswer({
  label,
  places,
  map,
  selectedId,
  onSelect,
}: {
  label: string;
  places: readonly MapPlace[];
  map: ReactNode;
  selectedId?: string;
  onSelect: (id: string) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{label}</PanelHeading>
      <div className="overflow-hidden rounded-control">{map}</div>
      <ol className="mt-3 space-y-2">
        {places.map((place) => (
          <li key={place.id}>
            <Button
              className="w-full"
              variant="navigation"
              selected={place.id === selectedId}
              onClick={() => {
                onSelect(place.id);
              }}
            >
              <Icon name="pin" />
              <span className="min-w-0">
                <span className="block font-medium">{place.name}</span>
                <span className="block text-xs text-text-muted">{place.description}</span>
              </span>
            </Button>
          </li>
        ))}
      </ol>
    </AssistantPanel>
  );
}

/** Tile geography stays outside the shared library; a host map is paired with equivalent text. */
export function GeoMap({
  attribution,
  ...props
}: React.ComponentProps<typeof MapAnswer> & { attribution: ReactNode }) {
  return (
    <div className="space-y-2">
      <MapAnswer {...props} />
      <p className="text-xs text-text-muted">{attribution}</p>
    </div>
  );
}
