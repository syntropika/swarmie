// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { mergeProps } from "@react-aria/utils";
import { useRef, useState } from "react";
import type { ComponentProps } from "react";
import { useFocusRing } from "react-aria";

import { cn } from "../classes";

export type ResizeHandleProps = Omit<ComponentProps<"div">, "role" | "tabIndex">;
/** A focusable window splitter exposes its current size and supports arrow-key resizing. */
export function ResizeHandle({
  "aria-orientation": orientation = "vertical",
  ...props
}: ResizeHandleProps) {
  const { focusProps, isFocused, isFocusVisible } = useFocusRing();
  // A focusable ARIA separator is a resizing control; a native hr would be a static divider.
  /* oxlint-disable jsx-a11y/prefer-tag-over-role */
  return (
    <div
      {...mergeProps(props, focusProps)}
      role="separator"
      tabIndex={0}
      aria-orientation={orientation}
      data-swarmie-focus=""
      data-focused={isFocused || undefined}
      data-focus-visible={isFocusVisible || undefined}
    />
  );
  /* oxlint-enable jsx-a11y/prefer-tag-over-role */
}

/** Shared bounded splitter behavior for navigation and whole content panels. */
export function PaneResize({
  className,
  width,
  onResize,
  minimum,
  maximum,
  reset,
  edge = "right",
  controls,
  label,
}: {
  readonly className?: string;
  readonly width: number;
  readonly onResize: (value: number) => void;
  readonly minimum: number;
  readonly maximum: number;
  readonly reset: number;
  readonly edge?: "left" | "right";
  readonly controls: string;
  readonly label: string;
}) {
  const origin = useRef<{ x: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const clamp = (value: number) => Math.max(minimum, Math.min(maximum, Math.round(value)));
  const direction = edge === "right" ? 1 : -1;
  return (
    <ResizeHandle
      className={cn(
        "relative touch-none outline-none select-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-transparent hover:after:bg-resize-active focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus focus-visible:after:bg-resize-active data-[dragging=true]:after:bg-resize-active",
        className,
      )}
      data-edge={edge}
      data-dragging={dragging}
      aria-label={label}
      aria-controls={controls}
      aria-valuemin={minimum}
      aria-valuemax={maximum}
      aria-valuenow={width}
      aria-valuetext={`${width} pixels`}
      onDoubleClick={() => {
        onResize(clamp(reset));
      }}
      onKeyDown={(event) => {
        let next: number;
        switch (event.key) {
          case "ArrowLeft": {
            next = width - 20 * direction;
            break;
          }
          case "ArrowRight": {
            next = width + 20 * direction;
            break;
          }
          case "Home": {
            next = minimum;
            break;
          }
          case "End": {
            next = maximum;
            break;
          }
          default: {
            return;
          }
        }
        event.preventDefault();
        onResize(clamp(next));
      }}
      onPointerDown={(event) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        origin.current = { x: event.clientX, width };
        setDragging(true);
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (origin.current && event.currentTarget.hasPointerCapture(event.pointerId)) {
          onResize(clamp(origin.current.width + (event.clientX - origin.current.x) * direction));
        }
      }}
      onPointerUp={(event) => {
        origin.current = null;
        setDragging(false);
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
      }}
      onPointerCancel={() => {
        origin.current = null;
        setDragging(false);
      }}
      onLostPointerCapture={() => {
        origin.current = null;
        setDragging(false);
      }}
    />
  );
}
