import { useEffect, useRef } from "react";
import type { Dispatch, SetStateAction } from "react";

export type DiagramView = { zoom: number; x: number; y: number };

/** SVG geometry preserves zoom and pan without style injection. Pointer capture keeps drags local. */
export function DiagramViewport({
  label,
  source,
  view,
  onViewChange,
}: {
  label: string;
  source: string | undefined;
  view: DiagramView;
  onViewChange: Dispatch<SetStateAction<DiagramView>>;
}) {
  const ref = useRef<SVGSVGElement>(null);
  const drag = useRef<{ x: number; y: number } | null>(null);
  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return undefined;
    }
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      onViewChange((current) => ({
        ...current,
        zoom: Math.max(0.5, Math.min(4, current.zoom + (event.deltaY < 0 ? 0.25 : -0.25))),
      }));
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      element.removeEventListener("wheel", onWheel);
    };
  }, [onViewChange]);
  return (
    <svg
      ref={ref}
      viewBox="0 0 600 400"
      className="w-full touch-none select-none"
      aria-label={label}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- Titled interactive SVG exposes an image role; a native image cannot hold pan geometry.
      role="img"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, y: event.clientY };
      }}
      onPointerMove={(event) => {
        const previous = drag.current;
        if (!previous) {
          return;
        }
        const { width } = event.currentTarget.getBoundingClientRect();
        if (width === 0) {
          return;
        }
        const ratio = 600 / width;
        onViewChange((current) => ({
          ...current,
          x: current.x + (event.clientX - previous.x) * ratio,
          y: current.y + (event.clientY - previous.y) * ratio,
        }));
        drag.current = { x: event.clientX, y: event.clientY };
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onLostPointerCapture={() => {
        drag.current = null;
      }}
    >
      <title>{label}</title>
      <g transform={`translate(${view.x} ${view.y}) scale(${view.zoom})`}>
        <image href={source} width="600" height="400" preserveAspectRatio="xMidYMid meet" />
      </g>
    </svg>
  );
}
