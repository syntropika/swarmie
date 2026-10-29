import { observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import type { Rect, Virtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
} from "react";
import type { ReactNode, Ref } from "react";

import { cn } from "../classes";

/** A stable reading position, independent of how many earlier rows are loaded. */
export type VirtualListPosition = { kind: "end" } | { kind: "anchor"; key: string; offset: number };

export type VirtualListHandle = {
  /** Reveal a loaded row; navigation controls can retain focus with focus: false. */
  scrollToKey: (key: string, options?: { focus?: boolean }) => void;
  scrollToEnd: () => void;
};

export type VirtualListProps<T> = {
  items: readonly T[];
  getItemKey: (item: T) => string;
  renderItem: (item: T, index: number) => ReactNode;
  label: string;
  estimateSize?: number;
  /** Applied once when rows first become available. Remount to restore another position. */
  initialPosition?: VirtualListPosition | undefined;
  onPositionChange?: (position: VirtualListPosition) => void;
  onVisibleKeysChange?: (keys: readonly string[]) => void;
  ref?: Ref<VirtualListHandle>;
  className?: string;
  header?: ReactNode;
  empty?: ReactNode;
  /** Follow appended or growing rows only while the reader is within 48px of the end. */
  followEnd?: boolean;
};

type ListVirtualizer = Virtualizer<HTMLElement, HTMLLIElement>;

type PendingAnchor = {
  key: string;
  offset: number;
  focus: boolean;
  previousOffset: number;
  stableFrames: number;
  frames: number;
};

const headerKey = "header";
const itemKeyPrefix = "item:";
const endThreshold = 48;

function dataKey(key: string | number | bigint): string | undefined {
  return typeof key === "string" && key.startsWith(itemKeyPrefix)
    ? key.slice(itemKeyPrefix.length)
    : undefined;
}

function sameKeys(previous: readonly string[], next: readonly string[]) {
  return previous.length === next.length && previous.every((key, index) => key === next[index]);
}

/** A measured, keyboard-scrollable list whose mounted rows stay bounded as data grows. */
export function VirtualList<T>({
  items,
  getItemKey,
  renderItem,
  label,
  estimateSize = 160,
  initialPosition,
  onPositionChange,
  onVisibleKeysChange,
  ref,
  className,
  header,
  empty,
  followEnd = false,
}: VirtualListProps<T>) {
  const viewportRef = useRef<HTMLElement>(null);
  const publishFrame = useRef<number | null>(null);
  const anchorFrame = useRef<number | null>(null);
  const pendingAnchor = useRef<PendingAnchor | null>(null);
  const resizeAnchor = useRef<((key: string, offset: number, focus: boolean) => boolean) | null>(
    null,
  );
  const followingBeforeResize = useRef(false);
  const initialized = useRef(false);
  const previousFirstKey = useRef<string | undefined>(undefined);
  const readingAnchor = useRef<Extract<VirtualListPosition, { kind: "anchor" }> | null>(null);
  const headerAtFold = useRef(false);
  const previousVisibleKeys = useRef<readonly string[]>([]);
  const headerCount = header === undefined || header === null ? 0 : 1;
  const keys = useMemo(
    () => [
      ...(headerCount ? [headerKey] : []),
      ...items.map((item) => `${itemKeyPrefix}${getItemKey(item)}`),
    ],
    [getItemKey, headerCount, items],
  );
  const keyIndexes = useMemo(() => new Map(keys.map((key, index) => [key, index])), [keys]);
  const itemKeyAtIndex = useCallback(
    (index: number) => {
      const key = keys[index];
      if (key === undefined) {
        throw new RangeError("Virtual list index is outside the loaded rows.");
      }
      return key;
    },
    [keys],
  );

  const publishPosition = useCallback(
    (instance: ListVirtualizer) => {
      if (publishFrame.current !== null) {
        cancelAnimationFrame(publishFrame.current);
      }
      // Measurements can happen during React's commit. Notify consumers in the next frame.
      publishFrame.current = requestAnimationFrame(() => {
        publishFrame.current = null;
        if (pendingAnchor.current !== null) {
          return;
        }
        const offset = instance.scrollOffset ?? 0;
        const bottom = offset + (instance.scrollRect?.height ?? 0);
        const visibleItems = instance
          .getVirtualItems()
          .filter(
            (item) => item.end > offset && item.start < bottom && dataKey(item.key) !== undefined,
          );
        const visibleKeys = visibleItems.flatMap((item) => {
          const key = dataKey(item.key);
          return key === undefined ? [] : [key];
        });
        if (!sameKeys(previousVisibleKeys.current, visibleKeys)) {
          previousVisibleKeys.current = visibleKeys;
          onVisibleKeysChange?.(visibleKeys);
        }
        headerAtFold.current = instance
          .getVirtualItems()
          .some((item) => item.key === headerKey && item.end > offset);
        const [first] = visibleItems;
        const firstKey = first === undefined ? undefined : dataKey(first.key);
        if (first !== undefined && firstKey !== undefined) {
          const anchor = {
            kind: "anchor",
            key: firstKey,
            offset: offset - first.start,
          } satisfies VirtualListPosition;
          readingAnchor.current = anchor;
          followingBeforeResize.current = instance.isAtEnd(endThreshold);
          onPositionChange?.(followingBeforeResize.current ? { kind: "end" } : anchor);
        }
      });
    },
    [onPositionChange, onVisibleKeysChange],
  );

  const observeViewport = useCallback(
    (instance: ListVirtualizer, onRectChange: (rect: Rect) => void) =>
      observeElementRect(instance, (rect) => {
        const previous = instance.scrollRect;
        const anchor = readingAnchor.current;
        const resized =
          previous !== null &&
          (previous.height !== rect.height || previous.width !== rect.width) &&
          pendingAnchor.current === null;
        const keepFollowing =
          resized && instance.options.followOnAppend !== false && followingBeforeResize.current;
        onRectChange(rect);
        // Row anchoring does not account for a smaller viewport. Reapply the settled
        // end position after the new rect, including when text reflows at a new width.
        if (keepFollowing) {
          instance.scrollToEnd({ behavior: "auto" });
        } else if (resized && anchor !== null) {
          resizeAnchor.current?.(anchor.key, anchor.offset, false);
        }
      }),
    [],
  );

  // Mutable layout methods must remain outside React Compiler memoization.
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useVirtualizer<HTMLElement, HTMLLIElement>({
    count: keys.length,
    getScrollElement: () => viewportRef.current,
    getItemKey: itemKeyAtIndex,
    estimateSize: () => estimateSize,
    overscan: 6,
    anchorTo: "end",
    followOnAppend: followEnd,
    // A negative threshold keeps prepend anchoring but disables automatic end pinning.
    scrollEndThreshold: followEnd ? endThreshold : -1,
    onChange: publishPosition,
    observeElementRect: observeViewport,
    // The library owns only measured height/transform geometry through its DOM refs.
    // All visual styling remains in Tailwind; no JSX style or component stylesheet is needed.
    directDomUpdates: true,
  });

  const cancelAnchor = useCallback(() => {
    pendingAnchor.current = null;
    if (anchorFrame.current !== null) {
      cancelAnimationFrame(anchorFrame.current);
      anchorFrame.current = null;
    }
  }, []);

  const restoreAnchor = useCallback(
    (key: string, offset: number, focus: boolean) => {
      cancelAnchor();
      const index = keyIndexes.get(`${itemKeyPrefix}${key}`);
      if (index === undefined) {
        return false;
      }
      followingBeforeResize.current = false;
      pendingAnchor.current = {
        key,
        offset,
        focus,
        previousOffset: -1,
        stableFrames: 0,
        frames: 0,
      };
      virtualizer.scrollToIndex(index, { align: "start", behavior: "auto" });

      const reconcile = () => {
        const pending = pendingAnchor.current;
        const viewport = viewportRef.current;
        if (pending === null || viewport === null) {
          return;
        }
        pending.frames += 1;
        const row = virtualizer
          .getVirtualItems()
          .find((item) => item.key === `${itemKeyPrefix}${pending.key}`);
        const element = virtualizer.elementsCache.get(`${itemKeyPrefix}${pending.key}`);
        if (row !== undefined && element !== undefined) {
          const targetOffset = Math.max(
            0,
            Math.min(row.start + pending.offset, viewport.scrollHeight - viewport.clientHeight),
          );
          if (Math.abs(viewport.scrollTop - targetOffset) > 1) {
            virtualizer.scrollToOffset(targetOffset, { behavior: "auto" });
          }
          pending.stableFrames =
            Math.abs(pending.previousOffset - targetOffset) <= 1 ? pending.stableFrames + 1 : 0;
          pending.previousOffset = targetOffset;
          if (pending.stableFrames >= 3) {
            readingAnchor.current = { kind: "anchor", key: pending.key, offset: pending.offset };
            followingBeforeResize.current = virtualizer.isAtEnd(endThreshold);
            if (pending.focus) {
              element.focus({ preventScroll: true });
            }
            cancelAnchor();
            publishPosition(virtualizer);
            return;
          }
        }
        // A removed target or an interrupted layout must not leave a permanent animation loop.
        if (pending.frames >= 120) {
          cancelAnchor();
          publishPosition(virtualizer);
          return;
        }
        anchorFrame.current = requestAnimationFrame(reconcile);
      };
      anchorFrame.current = requestAnimationFrame(reconcile);
      return true;
    },
    [cancelAnchor, keyIndexes, publishPosition, virtualizer],
  );

  useLayoutEffect(() => {
    // ResizeObserver retains its callback; use the restoration function from the latest commit.
    resizeAnchor.current = restoreAnchor;
    return () => {
      resizeAnchor.current = null;
    };
  }, [restoreAnchor]);

  const interruptRestore = useCallback(() => {
    followingBeforeResize.current = false;
    const wasRestoring = pendingAnchor.current !== null;
    cancelAnchor();
    if (wasRestoring && viewportRef.current !== null) {
      // Stop index reconciliation before the browser applies the reader's native scroll.
      virtualizer.scrollToOffset(viewportRef.current.scrollTop, { behavior: "auto" });
    }
    // Sample after the native gesture, including gestures that leave the viewport at end.
    publishPosition(virtualizer);
  }, [cancelAnchor, publishPosition, virtualizer]);

  const firstDataKey = keys[headerCount];
  useLayoutEffect(() => {
    const previous = previousFirstKey.current;
    previousFirstKey.current = firstDataKey;
    const anchor = readingAnchor.current;
    if (
      previous !== undefined &&
      previous !== firstDataKey &&
      keyIndexes.has(previous) &&
      headerAtFold.current &&
      anchor !== null &&
      pendingAnchor.current === null
    ) {
      // A loading header can remain or disappear with a prepend. Preserve the data row
      // below it instead of allowing the virtualizer to anchor to that header.
      restoreAnchor(anchor.key, anchor.offset, false);
    }
  }, [firstDataKey, keyIndexes, restoreAnchor]);

  useImperativeHandle(
    ref,
    () => ({
      scrollToKey: (key, options) => {
        restoreAnchor(key, 0, options?.focus ?? true);
      },
      scrollToEnd: () => {
        cancelAnchor();
        virtualizer.scrollToEnd({ behavior: "auto" });
      },
    }),
    [cancelAnchor, restoreAnchor, virtualizer],
  );

  useLayoutEffect(() => {
    if (initialized.current || items.length === 0) {
      return;
    }
    initialized.current = true;
    if (initialPosition?.kind === "anchor") {
      if (restoreAnchor(initialPosition.key, initialPosition.offset, false)) {
        return;
      }
      if (followEnd) {
        virtualizer.scrollToEnd({ behavior: "auto" });
      }
    } else if (initialPosition?.kind === "end") {
      virtualizer.scrollToEnd({ behavior: "auto" });
    }
    publishPosition(virtualizer);
  }, [followEnd, initialPosition, items.length, publishPosition, restoreAnchor, virtualizer]);

  useEffect(
    () => () => {
      cancelAnchor();
      if (publishFrame.current !== null) {
        cancelAnimationFrame(publishFrame.current);
      }
    },
    [cancelAnchor],
  );

  /* oxlint-disable jsx-a11y/no-noninteractive-tabindex, jsx-a11y/no-noninteractive-element-interactions -- A labelled native scroll region must be focusable for keyboard scrolling; input cancels an in-flight restore without preventing native behavior. */
  return (
    <section
      ref={viewportRef}
      aria-label={label}
      tabIndex={0}
      className={cn(
        "relative min-h-0 min-w-0 overflow-y-auto overscroll-contain outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus",
        className,
      )}
      onWheel={interruptRestore}
      onTouchStart={interruptRestore}
      onKeyDown={(event) => {
        if (
          ["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(event.key)
        ) {
          interruptRestore();
        }
      }}
    >
      <ol ref={virtualizer.containerRef} className="relative m-0 list-none p-0">
        {virtualizer.getVirtualItems().map((row) => {
          const index = row.index - headerCount;
          if (row.key === headerKey) {
            return (
              <li
                key={row.key}
                ref={virtualizer.measureElement}
                data-index={row.index}
                role="presentation"
                className="absolute top-0 left-0 w-full"
              >
                {header}
              </li>
            );
          }
          const item = items[index];
          if (item === undefined) {
            return null;
          }
          return (
            <li
              key={row.key}
              ref={virtualizer.measureElement}
              data-index={row.index}
              data-virtual-key={getItemKey(item)}
              aria-posinset={index + 1}
              aria-setsize={items.length}
              tabIndex={-1}
              className="absolute top-0 left-0 w-full outline-none"
            >
              {renderItem(item, index)}
            </li>
          );
        })}
      </ol>
      {items.length === 0 ? empty : null}
    </section>
  );
  /* oxlint-enable jsx-a11y/no-noninteractive-tabindex, jsx-a11y/no-noninteractive-element-interactions */
}
