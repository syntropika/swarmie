// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { ComponentProps } from "react";

export type ResizeHandleProps = Omit<ComponentProps<"div">, "role" | "tabIndex">;
/** A focusable window splitter exposes its current size and supports arrow-key resizing. */
export function ResizeHandle({
  "aria-orientation": orientation = "vertical",
  ...props
}: ResizeHandleProps) {
  // A focusable ARIA separator is a resizing control; a native hr would be a static divider.
  /* oxlint-disable jsx-a11y/prefer-tag-over-role */
  return <div {...props} role="separator" tabIndex={0} aria-orientation={orientation} />;
  /* oxlint-enable jsx-a11y/prefer-tag-over-role */
}
