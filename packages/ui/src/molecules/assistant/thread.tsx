import { useEffect, useId, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { Input } from "../../atoms/input";
import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from "../../atoms/tooltip";
import { cn } from "../../classes";
import { ConversationArrival } from "./conversation-motion";
import { AssistantPanel, PanelHeading, TooltipIconButton } from "./shared";

/** A provider-neutral conversation container with independently anchored composer. */
export function ThreadRoot({ children, className, ...props }: ComponentPropsWithRef<"section">) {
  return (
    <section
      {...props}
      className={cn("@container flex min-h-96 min-w-0 flex-col bg-canvas text-ink", className)}
    >
      {children}
    </section>
  );
}

/** Auto-scroll follows new content only while the user remains at the bottom. */
export function ThreadViewport({
  children,
  className,
  onPinnedChange,
  label,
  ref,
  onScroll,
  ...props
}: ComponentPropsWithRef<"div"> & { label: string; onPinnedChange?: (pinned: boolean) => void }) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  useEffect(() => {
    const viewport = viewportRef.current;
    const content = contentRef.current;
    if (!viewport || !content) {
      return undefined;
    }
    const observer = new ResizeObserver(() => {
      if (pinnedRef.current) {
        viewport.scrollTop = viewport.scrollHeight;
      }
    });
    observer.observe(content);
    return () => {
      observer.disconnect();
    };
  }, []);
  return (
    <div
      {...props}
      ref={(element) => {
        viewportRef.current = element;
        if (typeof ref === "function") {
          return ref(element);
        }
        if (ref) {
          ref.current = element;
        }
      }}
      role="log"
      aria-label={label}
      // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- Keyboard users need focus to scroll this bounded content region.
      tabIndex={0}
      className={cn(
        "min-h-0 flex-1 overflow-y-auto px-3 py-4 focus-visible:outline-2 focus-visible:outline-primary",
        className,
      )}
      onScroll={(event) => {
        onScroll?.(event);
        const element = event.currentTarget;
        const pinned = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
        if (pinned !== pinnedRef.current) {
          pinnedRef.current = pinned;
          onPinnedChange?.(pinned);
        }
      }}
    >
      <div ref={contentRef} className="mx-auto flex max-w-3xl flex-col gap-4">
        {children}
      </div>
    </div>
  );
}

export function ThreadComposer({ children }: { children: ReactNode }) {
  return (
    <div className="sticky bottom-0 z-10 bg-canvas px-3 pt-3 pb-4">
      <div className="mx-auto max-w-3xl">{children}</div>
    </div>
  );
}

export function EmptyThread({
  title,
  description,
  suggestions,
  onSelect,
  children,
}: {
  title: string;
  description: string;
  suggestions: readonly { id: string; title: string; detail: string }[];
  onSelect: (id: string) => void;
  children?: ReactNode;
}) {
  return (
    <div className="@container mx-auto flex max-w-3xl flex-1 flex-col justify-center gap-4 p-4">
      <div>
        <h2 className="text-title">{title}</h2>
        <p className="mt-3 max-w-prose text-body leading-body text-text-muted">{description}</p>
      </div>
      <ul className="grid gap-3 @lg:grid-cols-3">
        {suggestions.map((suggestion) => (
          <li key={suggestion.id}>
            <Button
              className="h-full w-full items-start"
              variant="navigation"
              onClick={() => {
                onSelect(suggestion.id);
              }}
            >
              <span>
                <span className="block font-medium">{suggestion.title}</span>
                <span className="mt-1 block text-xs text-text-muted">{suggestion.detail}</span>
              </span>
            </Button>
          </li>
        ))}
      </ul>
      {children}
    </div>
  );
}

export function ScrollAnchor({
  label,
  onClick,
  hidden = false,
}: {
  label: string;
  onClick: () => void;
  hidden?: boolean;
}) {
  if (hidden) {
    return null;
  }
  return (
    <Button variant="secondary" onClick={onClick}>
      <Icon name="chevron" />
      {label}
    </Button>
  );
}

export function CanvasSplit({
  conversation,
  canvas,
  label,
  closeLabel,
  onClose,
}: {
  conversation: ReactNode;
  canvas: ReactNode;
  label: string;
  closeLabel: string;
  onClose: () => void;
}) {
  return (
    <div className="@container">
      <div className="grid min-w-0 gap-3 @3xl:grid-cols-2">
        <div className="min-w-0">{conversation}</div>
        <aside className="min-w-0 rounded-panel border border-transparent bg-surface p-3">
          <PanelHeading
            actions={<TooltipIconButton label={closeLabel} icon="close" onClick={onClose} />}
          >
            {label}
          </PanelHeading>
          {canvas}
        </aside>
      </div>
    </div>
  );
}

export function ConnectionState({
  state,
  label,
  detail,
  retryLabel,
  onRetry,
}: {
  state: "connected" | "reconnecting" | "offline" | "resumed";
  label: string;
  detail: string;
  retryLabel: string;
  onRetry: () => void;
}) {
  if (state === "connected") {
    return null;
  }
  return (
    <AssistantPanel
      key={state}
      aria-live="polite"
      className="motion-safe:animate-assistant-receipt"
    >
      <div className="flex flex-wrap items-center gap-3">
        <Icon
          name={state === "reconnecting" ? "loading" : "world"}
          className={state === "reconnecting" ? "motion-safe:animate-assistant-spin" : undefined}
        />
        <Badge tone={state === "resumed" ? "success" : "attention"}>{label}</Badge>
        <span className="flex-1 text-ui text-text-muted">{detail}</span>
        {state === "offline" ? (
          <Button variant="secondary" onClick={onRetry}>
            {retryLabel}
          </Button>
        ) : null}
      </div>
    </AssistantPanel>
  );
}

export function SharedConversation({
  title,
  attribution,
  children,
  forkLabel,
  onFork,
}: {
  title: string;
  attribution: string;
  children: ReactNode;
  forkLabel: string;
  onFork: () => void;
}) {
  return (
    <article className="space-y-4">
      <header className="flex flex-wrap justify-between gap-3">
        <div>
          <h2 className="text-title">{title}</h2>
          <p className="text-ui text-text-muted">{attribution}</p>
        </div>
        <Button onClick={onFork}>{forkLabel}</Button>
      </header>
      {children}
    </article>
  );
}

export type SearchHit = {
  id: string;
  before: string;
  match: string;
  after: string;
  author: string;
  position?: number;
};

export function ConversationSearch({
  label,
  value,
  onChange,
  hits,
  onSelect,
  emptyLabel,
  activeIndex = 0,
  previousLabel = "Previous match",
  nextLabel = "Next match",
  onActiveIndexChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  hits: readonly SearchHit[];
  onSelect: (id: string) => void;
  emptyLabel: string;
  activeIndex?: number;
  onActiveIndexChange?: (index: number) => void;
  previousLabel?: string;
  nextLabel?: string;
}) {
  const index = Math.min(Math.max(activeIndex, 0), Math.max(0, hits.length - 1));
  const hit = hits[index];
  const select = (next: number) => {
    const match = hits[next];
    if (match) {
      onActiveIndexChange?.(next);
      onSelect(match.id);
    }
  };
  return (
    <AssistantPanel>
      <label className="flex flex-col gap-2 text-ui font-medium">
        {label}
        <Input
          type="search"
          value={value}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      </label>
      <div className="mt-2 flex items-center gap-2">
        <output className="text-xs tabular-nums">
          {hits.length ? index + 1 : 0} / {hits.length}
        </output>
        <TooltipIconButton
          label={previousLabel}
          icon="back"
          disabled={!hits.length}
          onClick={() => {
            select((index - 1 + hits.length) % hits.length);
          }}
        />
        <TooltipIconButton
          label={nextLabel}
          icon="arrow"
          disabled={!hits.length}
          onClick={() => {
            select((index + 1) % hits.length);
          }}
        />
      </div>
      <div className="mt-2 flex gap-3">
        {hit ? (
          <ConversationArrival
            key={hit.id}
            duration={200}
            motion="fade"
            className="min-w-0 flex-1 text-ui"
          >
            <p className="text-xs text-text-muted">{hit.author}</p>
            {hit.before}
            <mark className="rounded-pill bg-butter text-ink">{hit.match}</mark>
            {hit.after}
          </ConversationArrival>
        ) : (
          <p className="text-ui text-text-muted">{emptyLabel}</p>
        )}
        <svg
          viewBox="0 0 8 100"
          aria-hidden="true"
          className="h-24 w-2 shrink-0 rounded-full bg-surface-muted"
        >
          {hits.map((match, matchIndex) => (
            <rect
              key={match.id}
              x="0"
              y={Math.min(
                97,
                Math.max(0, match.position ?? (matchIndex / Math.max(1, hits.length - 1)) * 97),
              )}
              width="8"
              height="3"
              rx="1"
              className={cn(
                "transition-colors duration-200 motion-reduce:transition-none",
                matchIndex === index ? "fill-primary" : "fill-control-border",
              )}
            />
          ))}
        </svg>
      </div>
    </AssistantPanel>
  );
}

export type ThreadListItem = {
  id: string;
  title: string;
  detail: string;
  group: string;
  pinned?: boolean;
};

export function ThreadList({
  label,
  items,
  activeId,
  onSelect,
  actionLabel,
  onAction,
  pinnedLabel = "Pinned",
}: {
  label: string;
  items: readonly ThreadListItem[];
  activeId: string;
  onSelect: (id: string) => void;
  pinnedLabel?: string;
  actionLabel?: string;
  onAction?: (id: string) => void;
}) {
  const orderedItems = [
    ...items.filter((item) => item.pinned === true),
    ...items.filter((item) => item.pinned !== true),
  ];
  const groups = [
    ...new Set(orderedItems.map((item) => (item.pinned === true ? pinnedLabel : item.group))),
  ];
  return (
    <nav aria-label={label} className="space-y-3">
      {groups.map((group) => (
        <section key={group}>
          <h3 className="mb-2 px-3 text-xs font-medium text-text-muted">{group}</h3>
          <ul className="space-y-1">
            {orderedItems
              .filter((item) => (item.pinned === true ? pinnedLabel : item.group) === group)
              .map((item) => (
                <li key={item.id} className="flex items-center gap-1">
                  <Button
                    variant="navigation"
                    size="navigation"
                    selected={activeId === item.id}
                    className="min-w-0 flex-1"
                    onClick={() => {
                      onSelect(item.id);
                    }}
                  >
                    {item.pinned === true ? <Icon name="pin" /> : <Icon name="chat" />}
                    <span className="min-w-0">
                      <span className="block truncate" title={item.detail}>
                        {item.title}
                      </span>
                      <span className="block truncate text-xs text-text-muted">{item.detail}</span>
                    </span>
                  </Button>
                  {actionLabel !== undefined && onAction ? (
                    <TooltipIconButton
                      label={`${actionLabel}: ${item.title}`}
                      icon="menu"
                      onClick={() => {
                        onAction(item.id);
                      }}
                    />
                  ) : null}
                </li>
              ))}
          </ul>
        </section>
      ))}
    </nav>
  );
}

export function ThreadSearch({
  items,
  activeId,
  onSelect,
  searchLabel,
  ...props
}: React.ComponentProps<typeof ThreadList> & { searchLabel: string }) {
  const [query, setQuery] = useState("");
  const matches = items.filter((item) =>
    `${item.title} ${item.detail}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
  );
  const groups = [
    ...new Set(matches.filter((item) => item.pinned !== true).map((item) => item.group)),
  ];
  const visible = [
    ...matches.filter((item) => item.pinned === true),
    ...groups.flatMap((group) =>
      matches.filter((item) => item.pinned !== true && item.group === group),
    ),
  ];
  return (
    <div className="space-y-3">
      <label className="flex flex-col gap-2 text-ui">
        {searchLabel}
        <Input
          type="search"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
          }}
          onKeyDown={(event) => {
            if (
              event.nativeEvent.isComposing ||
              !visible.length ||
              (event.key !== "ArrowDown" && event.key !== "ArrowUp")
            ) {
              return;
            }
            event.preventDefault();
            const delta = event.key === "ArrowDown" ? 1 : -1;
            const found = visible.findIndex((item) => item.id === activeId);
            let from = found;
            if (from === -1) {
              from = delta > 0 ? -1 : 0;
            }
            const next = visible[(from + delta + visible.length) % visible.length];
            if (next) {
              onSelect(next.id);
            }
          }}
        />
      </label>
      <ThreadList {...props} items={visible} activeId={activeId} onSelect={onSelect} />
    </div>
  );
}

/** Native modal focus containment, Escape, and focus restoration. */
export function AssistantModal({
  open,
  onOpenChange,
  title,
  closeLabel,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  closeLabel: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) {
      return;
    }
    if (open && !dialog.open) {
      dialog.showModal();
    }
    if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);
  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onCancel={() => {
        onOpenChange(false);
      }}
      onClose={() => {
        onOpenChange(false);
      }}
      className="assistant-modal m-auto max-h-[90dvh] w-[min(48rem,calc(100vw-2rem))] overflow-auto rounded-panel border border-transparent bg-surface p-4 text-ink backdrop:bg-ink/40"
    >
      <PanelHeading
        actions={
          <TooltipIconButton
            label={closeLabel}
            icon="close"
            onClick={() => {
              onOpenChange(false);
            }}
          />
        }
      >
        <span id={titleId}>{title}</span>
      </PanelHeading>
      {children}
    </dialog>
  );
}

export function AssistantSidebar({
  title,
  children,
  closeLabel,
  onClose,
}: {
  title: string;
  children: ReactNode;
  closeLabel: string;
  onClose: () => void;
}) {
  return (
    <aside
      aria-label={title}
      className="min-w-0 rounded-panel border border-transparent bg-surface p-3"
    >
      <PanelHeading
        actions={<TooltipIconButton label={closeLabel} icon="close" onClick={onClose} />}
      >
        {title}
      </PanelHeading>
      {children}
    </aside>
  );
}

export function ThreadListSidebar({
  sidebar,
  children,
}: {
  sidebar: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="@container">
      <div className="grid min-w-0 gap-3 @3xl:grid-cols-[18rem_1fr]">
        <aside className="min-w-0 rounded-panel bg-rail p-3">{sidebar}</aside>
        <div className="min-w-0">{children}</div>
      </div>
    </div>
  );
}

export function AssistantLauncher({
  label,
  children,
  open: controlledOpen,
  onOpenChange,
  unread = 0,
  greeting,
  prompts,
  onPick,
  startLabel,
  onStart,
}: {
  label: string;
  children?: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  unread?: number;
  greeting?: string;
  prompts?: readonly string[];
  onPick?: (prompt: string) => void;
  startLabel?: string;
  onStart?: () => void;
}) {
  const [localOpen, setLocalOpen] = useState(false);
  const open = controlledOpen ?? localOpen;
  const panelId = useId();
  return (
    <div className="space-y-3">
      {open ? (
        <ConversationArrival duration={200} motion="menu">
          <section id={panelId} className="space-y-3 rounded-panel bg-surface p-3">
            {greeting === undefined ? null : <p className="text-ui font-medium">{greeting}</p>}
            {prompts?.map((prompt) => (
              <Button
                key={prompt}
                variant="navigation"
                disabled={!onPick}
                onClick={() => onPick?.(prompt)}
              >
                {prompt}
              </Button>
            ))}
            {onStart && startLabel !== undefined ? (
              <Button onClick={onStart}>{startLabel}</Button>
            ) : null}
            {children}
          </section>
        </ConversationArrival>
      ) : null}
      <Button
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => {
          setLocalOpen(!open);
          onOpenChange?.(!open);
        }}
      >
        <span className="grid">
          <Icon
            name="chat"
            className={cn(
              "col-start-1 row-start-1 transition-all duration-200 motion-reduce:transition-none",
              open && "rotate-90 opacity-0",
            )}
          />
          <Icon
            name="close"
            className={cn(
              "col-start-1 row-start-1 transition-all duration-200 motion-reduce:transition-none",
              !open && "-rotate-90 opacity-0",
            )}
          />
        </span>
        {label}
        {!open && unread > 0 ? <Badge>{unread}</Badge> : null}
      </Button>
    </div>
  );
}

export function AssistantSettings({
  title,
  children,
  temperature,
  temperatureLabel,
  onTemperatureChange,
  capabilities,
  onCapabilityChange,
}: {
  title: string;
  children: ReactNode;
  temperature?: number;
  temperatureLabel?: string;
  onTemperatureChange?: (value: number) => void;
  capabilities?: readonly { id: string; label: string; detail: string; enabled: boolean }[];
  onCapabilityChange?: (id: string, enabled: boolean) => void;
}) {
  return (
    <AssistantPanel>
      <PanelHeading>{title}</PanelHeading>
      <div className="space-y-4">
        {children}
        {temperature === undefined ? null : (
          <label className="flex flex-col gap-2 text-ui">
            <span className="flex justify-between">
              {temperatureLabel}
              <output>{temperature.toFixed(1)}</output>
            </span>
            <input
              type="range"
              min={0}
              max={2}
              step={0.1}
              value={temperature}
              disabled={!onTemperatureChange}
              onChange={(event) => {
                onTemperatureChange?.(Number(event.target.value));
              }}
              className="w-full accent-primary"
            />
          </label>
        )}
        {capabilities?.map((capability) => (
          <div key={capability.id} className="flex items-center gap-3">
            <span className="flex-1">
              <span className="block text-ui">{capability.label}</span>
              <span className="block text-xs text-text-muted">{capability.detail}</span>
            </span>
            <Button
              role="switch"
              aria-label={capability.label}
              aria-checked={capability.enabled}
              disabled={!onCapabilityChange}
              variant="ghost"
              className={cn(
                "h-6 w-10 justify-start p-1 transition-colors duration-200",
                capability.enabled ? "bg-primary" : "bg-surface-muted",
              )}
              onClick={() => {
                onCapabilityChange?.(capability.id, !capability.enabled);
              }}
            >
              <span
                className={cn(
                  "size-4 rounded-full bg-surface transition-transform duration-200 motion-reduce:transition-none",
                  capability.enabled && "translate-x-4",
                )}
              />
            </Button>
          </div>
        ))}
      </div>
    </AssistantPanel>
  );
}

export function AssistantOnboarding({
  steps,
  index,
  nextLabel,
  backLabel,
  finishLabel,
  onChange,
  onFinish,
  onSkip,
  skipLabel,
}: {
  steps: readonly { id: string; title: string; description: string; content?: ReactNode }[];
  index: number;
  nextLabel: string;
  backLabel: string;
  finishLabel: string;
  onChange: (index: number) => void;
  onFinish: () => void;
  onSkip?: () => void;
  skipLabel?: string;
}) {
  const step = steps[index];
  if (!step) {
    return null;
  }
  const last = index === steps.length - 1;
  return (
    <AssistantPanel>
      <p className="mb-3 text-xs text-text-muted tabular-nums">
        {index + 1} / {steps.length}
      </p>
      <div key={step.id} className="motion-safe:animate-assistant-receipt">
        <h2 className="text-title">{step.title}</h2>
        <p className="my-3 text-ui text-text-muted">{step.description}</p>
        {step.content}
      </div>
      <div aria-hidden="true" className="mt-3 flex gap-1.5">
        {steps.map((item, at) => (
          <span
            key={item.id}
            className={cn(
              "h-1 rounded-full transition-all duration-300 motion-reduce:transition-none",
              at === index ? "w-4 bg-primary" : "w-1 bg-control-border",
            )}
          />
        ))}
      </div>
      {onSkip && skipLabel !== undefined ? (
        <Button variant="ghost" onClick={onSkip}>
          {skipLabel}
        </Button>
      ) : null}
      <div className="mt-5 flex justify-between gap-3">
        <Button
          variant="ghost"
          disabled={index === 0}
          onClick={() => {
            onChange(index - 1);
          }}
        >
          {backLabel}
        </Button>
        <Button
          onClick={() => {
            if (last) {
              onFinish();
            } else {
              onChange(index + 1);
            }
          }}
        >
          {last ? finishLabel : nextLabel}
        </Button>
      </div>
    </AssistantPanel>
  );
}

export function MobileComposer({
  label,
  children,
  actions,
  keyboardOpen = false,
}: {
  label: string;
  children: ReactNode;
  actions: ReactNode;
  keyboardOpen?: boolean;
}) {
  return (
    <section
      aria-label={label}
      className={cn(
        "sticky bottom-0 rounded-t-panel border border-transparent bg-surface p-3",
        !keyboardOpen && "pb-safe",
      )}
    >
      {keyboardOpen ? null : (
        <ConversationArrival
          duration={200}
          motion="fade"
          className="mb-3 flex gap-2 overflow-x-auto"
        >
          {actions}
        </ConversationArrival>
      )}
      {children}
    </section>
  );
}

export type ConversationMapItem = { id: string; label: string; speaker: string; visible: boolean };

export function ConversationMap({
  label,
  items,
  activeId,
  onSelect,
}: {
  label: string;
  items: readonly ConversationMapItem[];
  activeId: string;
  onSelect: (id: string) => void;
}) {
  const refs = useRef(new Map<string, HTMLButtonElement>());
  const active = items.some((item) => item.id === activeId) ? activeId : items[0]?.id;
  return (
    <nav aria-label={label}>
      <TooltipProvider>
        <ol className="flex flex-col items-start gap-1">
          {items.map((item, index) => (
            <li key={item.id}>
              <TooltipRoot>
                <TooltipTrigger asChild>
                  <Button
                    ref={(element) => {
                      if (element) {
                        refs.current.set(item.id, element);
                      } else {
                        refs.current.delete(item.id);
                      }
                    }}
                    variant="position"
                    tabIndex={active === item.id ? 0 : -1}
                    aria-current={item.id === activeId ? "location" : undefined}
                    onKeyDown={(event) => {
                      let next = index;
                      if (event.key === "ArrowDown") {
                        next = Math.min(items.length - 1, index + 1);
                      } else if (event.key === "ArrowUp") {
                        next = Math.max(0, index - 1);
                      } else if (event.key === "Home") {
                        next = 0;
                      } else if (event.key === "End") {
                        next = items.length - 1;
                      } else {
                        return;
                      }
                      event.preventDefault();
                      const target = items[next];
                      if (target) {
                        refs.current.get(target.id)?.focus();
                        onSelect(target.id);
                      }
                    }}
                    onClick={() => {
                      onSelect(item.id);
                    }}
                  >
                    <span
                      aria-hidden="true"
                      className={cn(
                        "h-1 rounded-full transition-all duration-200 motion-reduce:transition-none",
                        item.visible ? "w-6 bg-primary" : "w-4 bg-control-border",
                      )}
                    />
                    <span className="sr-only">
                      {item.speaker}: {item.label}
                    </span>
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="right">
                  {item.speaker}: {item.label}
                </TooltipContent>
              </TooltipRoot>
            </li>
          ))}
        </ol>
      </TooltipProvider>
    </nav>
  );
}

export function FollowUpSuggestions({
  label,
  suggestions,
  onSelect,
  running = false,
  empty = false,
}: {
  running?: boolean;
  empty?: boolean;
  label: string;
  suggestions: readonly { id: string; text: string }[];
  onSelect: (id: string) => void;
}) {
  if (running || empty || suggestions.length === 0) {
    return null;
  }
  return (
    <fieldset aria-label={label} className="flex gap-2 overflow-x-auto">
      {suggestions.map((suggestion) => (
        <Button
          key={suggestion.id}
          variant="secondary"
          className="shrink-0"
          onClick={() => {
            onSelect(suggestion.id);
          }}
        >
          {suggestion.text}
        </Button>
      ))}
    </fieldset>
  );
}
