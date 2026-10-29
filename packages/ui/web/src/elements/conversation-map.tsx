import type { ComponentProps, KeyboardEvent } from "react";
import { useLayoutEffect, useMemo, useRef, useState } from "react";

import { Button } from "../atoms/button";
import { Tooltip } from "../atoms/tooltip";
import { cn } from "../classes";
import {
  POSITION_RAIL_MARKER_HEIGHT,
  positionRailWindowIndexes,
} from "./conversation-map-geometry";

export type ConversationMapEntry = {
  id: string;
  title: string;
  preview?: string;
  role?: "user" | "assistant";
};
const noVisibleIds: readonly string[] = [];

/** A bounded positional window follows the transcript without owning its history or scroll state. */
export function ConversationMap({
  entries,
  activeId,
  visibleIds = noVisibleIds,
  onSelect,
  side = "right",
  className,
  ...props
}: Omit<ComponentProps<"nav">, "onSelect" | "children"> & {
  entries: readonly ConversationMapEntry[];
  activeId?: string | undefined;
  visibleIds?: readonly string[] | undefined;
  onSelect?: ((id: string) => void) | undefined;
  side?: "left" | "right";
}) {
  const root = useRef<HTMLElement | null>(null);
  const scroller = useRef<HTMLDivElement | null>(null);
  const markers = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocus = useRef<string | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [viewport, setViewport] = useState({
    offset: 0,
    height: 300,
    markerHeight: POSITION_RAIL_MARKER_HEIGHT,
  });
  const indexesById = useMemo(
    () => new Map(entries.map((entry, index) => [entry.id, index])),
    [entries],
  );
  const indexes = useMemo(
    () =>
      positionRailWindowIndexes(
        entries.length,
        indexesById,
        viewport.offset,
        viewport.height,
        viewport.markerHeight,
        activeId,
        focusedId,
        hoveredId,
      ),
    [entries.length, indexesById, viewport, activeId, focusedId, hoveredId],
  );
  const tabId = focusedId ?? activeId ?? entries[0]?.id;
  useLayoutEffect(() => {
    root.current?.style.setProperty("--conversation-map-count", String(entries.length));
  }, [entries.length]);
  useLayoutEffect(() => {
    const element = scroller.current;
    if (element === null) {
      return undefined;
    }
    let frame: number | null = null;
    function measure() {
      frame = null;
      if (element === null) {
        return;
      }
      const measured = markers.current.values().next().value?.offsetHeight ?? 0;
      const next = {
        offset: element.scrollTop,
        height: element.clientHeight,
        markerHeight: measured > 0 ? measured : POSITION_RAIL_MARKER_HEIGHT,
      };
      setViewport((previous) =>
        previous.offset === next.offset &&
        previous.height === next.height &&
        previous.markerHeight === next.markerHeight
          ? previous
          : next,
      );
    }
    function schedule() {
      frame ??= window.requestAnimationFrame(measure);
    }
    const observer = new ResizeObserver(schedule);
    observer.observe(element);
    const touch = window.matchMedia("(max-width: 767px), (pointer: coarse)");
    touch.addEventListener("change", schedule);
    element.addEventListener("scroll", schedule, { passive: true });
    measure();
    return () => {
      observer.disconnect();
      touch.removeEventListener("change", schedule);
      element.removeEventListener("scroll", schedule);
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
    };
  }, []);
  useLayoutEffect(() => {
    const element = scroller.current;
    const id = pendingFocus.current ?? activeId;
    const index = id === undefined ? undefined : indexesById.get(id);
    if (element === null || index === undefined || element.clientHeight === 0) {
      return;
    }
    if (focusedId !== null && pendingFocus.current === null) {
      return;
    }
    if (!indexes.includes(index)) {
      return;
    }
    const top = index * viewport.markerHeight;
    if (
      top < element.scrollTop ||
      top + viewport.markerHeight > element.scrollTop + element.clientHeight
    ) {
      element.scrollTop = Math.max(0, top - (element.clientHeight - viewport.markerHeight) / 2);
    }
    if (pendingFocus.current !== null) {
      const marker = markers.current.get(pendingFocus.current);
      if (marker) {
        pendingFocus.current = null;
        marker.focus({ preventScroll: true });
      }
    }
  }, [activeId, indexesById, indexes, focusedId, viewport.markerHeight]);
  function navigate(event: KeyboardEvent, index: number) {
    let next = index;
    if (event.key === "ArrowDown") {
      next += 1;
    } else if (event.key === "ArrowUp") {
      next -= 1;
    } else if (event.key === "Home") {
      next = 0;
    } else if (event.key === "End") {
      next = entries.length - 1;
    } else {
      return;
    }
    event.preventDefault();
    const target = entries[Math.max(0, Math.min(entries.length - 1, next))];
    if (target) {
      pendingFocus.current = target.id;
      setFocusedId(target.id);
    }
  }
  return (
    <nav
      {...props}
      ref={root}
      aria-label="Conversation map"
      data-slot="conversation-map"
      className={cn("conversation-map", className)}
      onPointerLeave={() => {
        setHoveredId(null);
      }}
    >
      <div
        ref={scroller}
        className="conversation-map__marks"
        onWheel={(event) => {
          event.stopPropagation();
        }}
        onTouchMove={(event) => {
          event.stopPropagation();
        }}
      >
        <ul className="conversation-map__space m-0 list-none p-0" aria-label="Messages">
          {indexes.map((index) => {
            const entry = entries[index];
            if (!entry) {
              return null;
            }
            return (
              <li
                key={entry.id}
                className="conversation-map__item"
                data-hovered={hoveredId === entry.id ? "true" : undefined}
                aria-posinset={index + 1}
                aria-setsize={entries.length}
              >
                <Tooltip delay={120} isOpen={hoveredId === entry.id || focusedId === entry.id}>
                  <Button
                    ref={(element) => {
                      if (element) {
                        markers.current.set(entry.id, element);
                        element.style.setProperty("--conversation-map-position", String(index));
                      } else {
                        markers.current.delete(entry.id);
                      }
                    }}
                    variant="ghost"
                    isIconOnly
                    className="conversation-map__marker justify-start rounded-none p-0"
                    aria-label={`${index + 1} of ${entries.length}: ${entry.title}`}
                    aria-description="Use Arrow Up and Arrow Down to preview messages. Press Enter to jump."
                    aria-current={activeId === entry.id ? "step" : false}
                    data-visible={visibleIds.includes(entry.id) ? "true" : undefined}
                    render={(nativeProps) => (
                      <button
                        {...nativeProps}
                        type="button"
                        tabIndex={tabId === entry.id ? 0 : -1}
                      />
                    )}
                    onPointerEnter={() => {
                      setHoveredId(entry.id);
                    }}
                    onFocus={() => {
                      setFocusedId(entry.id);
                    }}
                    onBlur={() => {
                      setFocusedId(null);
                    }}
                    onKeyDown={(event) => {
                      navigate(event, index);
                    }}
                    onPress={() => onSelect?.(entry.id)}
                  >
                    <span className="conversation-map__tick" aria-hidden="true" />
                  </Button>
                  <Tooltip.Content placement={side} className="conversation-map__preview">
                    <span className="conversation-map__label">{authorLabel(entry)}</span>
                    <p className="conversation-map__copy">{entry.preview ?? entry.title}</p>
                  </Tooltip.Content>
                </Tooltip>
              </li>
            );
          })}
        </ul>
      </div>
    </nav>
  );
}

function authorLabel(entry: ConversationMapEntry): string {
  if (entry.role === "user") {
    return "You";
  }
  if (entry.role === "assistant") {
    return "Assistant";
  }
  return entry.title;
}
