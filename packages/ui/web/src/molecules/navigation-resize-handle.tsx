import { useState } from "react";

import { ResizeHandle } from "../atoms/resize-handle";
import { NAVIGATION_MAX_TOTAL_WIDTH, NAVIGATION_MIN_TOTAL_WIDTH } from "./navigation-layout";

type NavigationResizeHandleProps = {
  label: string;
  totalWidth: number;
  origin: () => number;
  onResize: (totalWidth: number) => void;
};

/** Pointer capture and separator keys resize measured layout, rather than presentation styles. */
export function NavigationResizeHandle({
  label,
  totalWidth,
  origin,
  onResize,
}: NavigationResizeHandleProps) {
  const [dragging, setDragging] = useState(false);
  return (
    <ResizeHandle
      aria-label={label}
      aria-valuemin={NAVIGATION_MIN_TOTAL_WIDTH}
      aria-valuemax={NAVIGATION_MAX_TOTAL_WIDTH}
      aria-valuenow={Math.round(totalWidth)}
      data-dragging={dragging}
      className="absolute inset-y-0 -right-2 z-10 m-0 h-full w-4 cursor-col-resize touch-none border-0 bg-transparent select-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:bg-navigation-divider hover:after:bg-resize-active focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus focus-visible:after:bg-resize-active data-[dragging=true]:after:bg-resize-active"
      onPointerDown={(event) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          onResize(event.clientX - origin());
        }
      }}
      onPointerUp={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
        setDragging(false);
      }}
      onLostPointerCapture={() => {
        setDragging(false);
      }}
      onPointerCancel={() => {
        setDragging(false);
      }}
      onKeyDown={(event) => {
        let nextWidth: number;
        switch (event.key) {
          case "ArrowLeft": {
            nextWidth = Math.max(NAVIGATION_MIN_TOTAL_WIDTH, totalWidth - 8);
            break;
          }
          case "ArrowRight": {
            nextWidth = Math.min(NAVIGATION_MAX_TOTAL_WIDTH, totalWidth + 8);
            break;
          }
          case "Home": {
            nextWidth = NAVIGATION_MIN_TOTAL_WIDTH;
            break;
          }
          case "End": {
            nextWidth = NAVIGATION_MAX_TOTAL_WIDTH;
            break;
          }
          default: {
            return;
          }
        }
        event.preventDefault();
        onResize(nextWidth);
      }}
    />
  );
}
