// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { ComponentProps } from "react";

type ResizeHandleProps = Omit<ComponentProps<"div">, "role" | "tabIndex" | "className"> & {
  variant: "sidebar" | "panel";
};
/** A focusable window splitter exposes its current size and supports arrow-key resizing. */
export function ResizeHandle({ variant, ...props }: ResizeHandleProps) {
  // A focusable ARIA separator is a resizing control; a native hr would be a static divider.
  /* oxlint-disable jsx-a11y/prefer-tag-over-role */
  return (
    <div
      {...props}
      className={variant === "sidebar" ? "sidebar-resize" : "panel-resize"}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
    />
  );
  /* oxlint-enable jsx-a11y/prefer-tag-over-role */
}
