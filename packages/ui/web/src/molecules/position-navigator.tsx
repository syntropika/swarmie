import { defaultRangeExtractor, useVirtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import type { KeyboardEvent } from "react";

import { Button } from "../atoms/button";
import { cn } from "../classes";

export type PositionNavigatorItem = {
  id: string;
  label: string;
  preview: string;
};

export type PositionNavigatorProps = {
  items: readonly PositionNavigatorItem[];
  activeId?: string | undefined;
  /** All positions intersecting the consumer's viewport, excluding overscan. */
  visibleIds?: readonly string[];
  onSelect: (id: string) => void;
  label: string;
  className?: string;
};

const desktopQuery = "(min-width: 768px)";

function subscribeDesktop(onChange: () => void) {
  const query = window.matchMedia(desktopQuery);
  query.addEventListener("change", onChange);
  return () => {
    query.removeEventListener("change", onChange);
  };
}

function isDesktop() {
  return window.matchMedia(desktopQuery).matches;
}

function isDesktopOnServer() {
  return false;
}

type PositionPreviewProps = {
  item: PositionNavigatorItem;
  id: string;
  getAnchor: () => HTMLButtonElement | undefined;
  getViewport: () => HTMLDivElement | null;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
};

/** The browser top layer keeps previews outside the rail's scroll clipping. */
function PositionPreview({
  item,
  id,
  getAnchor,
  getViewport,
  onPointerEnter,
  onPointerLeave,
}: PositionPreviewProps) {
  const previewRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const preview = previewRef.current;
    if (!preview) {
      return undefined;
    }
    preview.showPopover();

    function positionPreview() {
      const anchor = getAnchor();
      const viewport = getViewport();
      if (!anchor || !preview || !viewport) {
        return;
      }
      const anchorBounds = anchor.getBoundingClientRect();
      const viewportBounds = viewport.getBoundingClientRect();
      const center = anchorBounds.top + anchorBounds.height / 2;
      if (center < viewportBounds.top || center > viewportBounds.bottom) {
        if (preview.matches(":popover-open")) {
          preview.hidePopover();
        }
        return;
      }
      if (!preview.matches(":popover-open")) {
        preview.showPopover();
      }
      const previewBounds = preview.getBoundingClientRect();
      // These coordinates are measured overlay geometry, not component appearance.
      preview.style.left = `${Math.max(8, Math.min(anchorBounds.right + 8, window.innerWidth - previewBounds.width - 8))}px`;
      preview.style.top = `${Math.max(8, Math.min(center - previewBounds.height / 2, window.innerHeight - previewBounds.height - 8))}px`;
    }

    positionPreview();
    window.addEventListener("resize", positionPreview);
    window.addEventListener("scroll", positionPreview, true);
    const observer = new ResizeObserver(positionPreview);
    const viewport = getViewport();
    if (viewport !== null) {
      observer.observe(viewport);
    }
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", positionPreview);
      window.removeEventListener("scroll", positionPreview, true);
    };
  }, [getAnchor, getViewport]);

  return (
    <div
      ref={previewRef}
      id={id}
      role="tooltip"
      popover="manual"
      className="fixed inset-auto m-0 w-50 max-w-[calc(100vw-1rem)] rounded-panel border-0 bg-surface px-2 py-1.5 text-left text-ui text-ink shadow-overlay"
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
    >
      <p className="line-clamp-2 font-medium break-words">{item.label}</p>
      {item.preview ? (
        <p className="mt-1 line-clamp-3 [overflow-wrap:anywhere] whitespace-pre-wrap text-text-muted">
          {/* Keep the bounded preview intact at a Unicode scalar boundary. */}
          {item.preview.slice(0, 140).replace(/[\uD800-\uDBFF]$/u, "")}
        </p>
      ) : null}
    </div>
  );
}

function PositionRail({ items, activeId, visibleIds, onSelect, label }: PositionNavigatorProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const buttonsRef = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusRef = useRef<string | null>(null);
  const closeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [dismissedId, setDismissedId] = useState<string | null>(null);
  const previewId = useId();
  const indexes = useMemo(() => new Map(items.map((item, index) => [item.id, index])), [items]);
  const visible = useMemo(() => new Set(visibleIds), [visibleIds]);
  const activeIndex = activeId === undefined ? undefined : indexes.get(activeId);
  const focusedIndex = focusedId === null ? undefined : indexes.get(focusedId);
  const hoveredIndex = hoveredId === null ? undefined : indexes.get(hoveredId);
  const targetId = hoveredId ?? focusedId;
  const previewIndex =
    targetId !== null && targetId !== dismissedId ? indexes.get(targetId) : undefined;
  const previewItem = previewIndex === undefined ? undefined : items[previewIndex];
  const tabStopIndex = focusedIndex ?? activeIndex ?? 0;
  const tabStopId = items[tabStopIndex]?.id;
  const pinnedIndexes = useMemo(
    () =>
      [activeIndex, focusedIndex, hoveredIndex, tabStopIndex].filter(
        (index) => index !== undefined,
      ),
    [activeIndex, focusedIndex, hoveredIndex, tabStopIndex],
  );
  const getItemKey = useCallback(
    (index: number) => {
      const item = items[index];
      if (!item) {
        throw new RangeError("Position navigator index is outside the item collection.");
      }
      return item.id;
    },
    [items],
  );
  // oxlint-disable-next-line react/incompatible-library -- TanStack's mutable geometry must be read each render; no virtualizer methods or rows are passed to memoized consumers.
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    getItemKey,
    estimateSize: () => 12,
    overscan: 6,
    rangeExtractor: useCallback(
      (range) =>
        [...new Set([...defaultRangeExtractor(range), ...pinnedIndexes])].toSorted((a, b) => a - b),
      [pinnedIndexes],
    ),
    // The library owns measured spacer height and row transforms without JSX styles.
    directDomUpdates: true,
  });

  useEffect(() => {
    if (activeIndex !== undefined) {
      virtualizer.scrollToIndex(activeIndex, { align: "auto" });
    }
  }, [activeIndex, virtualizer]);

  useLayoutEffect(() => {
    if (focusedId === null || pendingFocusRef.current !== focusedId) {
      return;
    }
    buttonsRef.current.get(focusedId)?.focus({ preventScroll: true });
    pendingFocusRef.current = null;
  }, [focusedId]);

  useEffect(
    () => () => {
      if (closeTimerRef.current) {
        clearTimeout(closeTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!previewItem) {
      return undefined;
    }
    const itemId = previewItem.id;
    function dismissPreview(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        setDismissedId(itemId);
      }
    }
    document.addEventListener("keydown", dismissPreview);
    return () => {
      document.removeEventListener("keydown", dismissPreview);
    };
  }, [previewItem]);

  function showPreview(id: string) {
    if (closeTimerRef.current) {
      clearTimeout(closeTimerRef.current);
    }
    setHoveredId(id);
    setDismissedId(null);
  }

  function leavePreview() {
    if (closeTimerRef.current) {
      clearTimeout(closeTimerRef.current);
    }
    closeTimerRef.current = setTimeout(() => {
      closeTimerRef.current = null;
      setHoveredId(null);
    }, 100);
  }

  function moveFocus(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let nextIndex: number;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowRight": {
        nextIndex = Math.min(items.length - 1, index + 1);
        break;
      }
      case "ArrowUp":
      case "ArrowLeft": {
        nextIndex = Math.max(0, index - 1);
        break;
      }
      case "Home": {
        nextIndex = 0;
        break;
      }
      case "End": {
        nextIndex = items.length - 1;
        break;
      }
      default: {
        return;
      }
    }
    event.preventDefault();
    const item = items[nextIndex];
    if (!item) {
      return;
    }
    pendingFocusRef.current = item.id;
    setFocusedId(item.id);
    setHoveredId(null);
    setDismissedId(null);
    virtualizer.scrollToIndex(nextIndex, { align: "auto" });
  }

  const getPreviewAnchor = useCallback(
    () => (previewItem ? buttonsRef.current.get(previewItem.id) : undefined),
    [previewItem],
  );
  const getPreviewViewport = useCallback(() => scrollRef.current, []);

  return (
    <nav aria-label={label} className="relative flex max-h-full min-h-0 w-11 flex-col">
      <div
        ref={scrollRef}
        className="max-h-[45dvh] min-h-0 scrollbar-none overflow-x-hidden overflow-y-auto overscroll-contain"
      >
        <ol ref={virtualizer.containerRef} className="relative w-11">
          {virtualizer.getVirtualItems().map((row) => {
            const item = items[row.index];
            if (!item) {
              return null;
            }
            const selected = item.id === activeId;
            const inView = visible.has(item.id);
            const distance =
              previewIndex === undefined ? undefined : Math.abs(row.index - previewIndex);
            return (
              <li
                key={item.id}
                ref={virtualizer.measureElement}
                data-index={row.index}
                aria-posinset={row.index + 1}
                aria-setsize={items.length}
                className="absolute top-0 left-0 flex h-3 w-11"
              >
                <Button
                  ref={(node) => {
                    if (node) {
                      buttonsRef.current.set(item.id, node);
                    } else {
                      buttonsRef.current.delete(item.id);
                    }
                  }}
                  variant="position"
                  size="position"
                  className="group"
                  data-visible={inView || selected ? "true" : undefined}
                  aria-label={item.label}
                  aria-current={selected ? "location" : undefined}
                  aria-describedby={previewItem?.id === item.id ? previewId : undefined}
                  tabIndex={tabStopId === item.id ? 0 : -1}
                  onClick={() => {
                    onSelect(item.id);
                  }}
                  onKeyDown={(event) => {
                    moveFocus(event, row.index);
                  }}
                  onFocus={() => {
                    setFocusedId(item.id);
                    setDismissedId(null);
                  }}
                  onBlur={() => {
                    setFocusedId(null);
                  }}
                  onPointerEnter={() => {
                    showPreview(item.id);
                  }}
                  onPointerLeave={leavePreview}
                >
                  <span
                    aria-hidden="true"
                    className={cn(
                      "h-0.5 rounded-full transition-all duration-150 motion-reduce:transition-none",
                      distance === 0 && "w-8",
                      distance === 1 && "w-6",
                      distance === 2 && "w-4",
                      distance === 3 && "w-3",
                      (distance === undefined || distance > 3) && "w-2",
                      selected && "bg-primary",
                      !selected && (inView ? "bg-text-muted" : "bg-text-muted/40"),
                    )}
                  />
                </Button>
              </li>
            );
          })}
        </ol>
      </div>
      {previewItem ? (
        <PositionPreview
          item={previewItem}
          id={previewId}
          getAnchor={getPreviewAnchor}
          getViewport={getPreviewViewport}
          onPointerEnter={() => {
            showPreview(previewItem.id);
          }}
          onPointerLeave={leavePreview}
        />
      ) : null}
    </nav>
  );
}

/** Navigate consumer-owned positions on desktop; the consumer changes the target document's scroll. */
export function PositionNavigator(props: PositionNavigatorProps) {
  const desktop = useSyncExternalStore(subscribeDesktop, isDesktop, isDesktopOnServer);
  const { items, className } = props;

  if (!desktop || items.length === 0) {
    return null;
  }

  return (
    <div className={className}>
      <PositionRail {...props} />
    </div>
  );
}
