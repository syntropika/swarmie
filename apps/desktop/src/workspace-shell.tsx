// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";

/** Only bounded numeric resize dimensions enter the stylesheet's layout variables. */
export function WorkspaceShell({
  sidebar,
  settings,
  panelWidth,
  sidebarWidth,
  children,
}: {
  sidebar: boolean;
  settings: boolean;
  panelWidth: number;
  sidebarWidth: number;
  children: ReactNode;
}) {
  const element = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    // Continuous pointer resizing cannot be expressed by a finite set of Tailwind classes.
    element.current?.style.setProperty(
      "--panel-width",
      `${Math.max(300, Math.min(800, panelWidth))}px`,
    );
    element.current?.style.setProperty(
      "--sidebar-width",
      `${Math.max(220, Math.min(360, sidebarWidth))}px`,
    );
  }, [panelWidth, sidebarWidth]);
  return (
    <div
      ref={element}
      className="app-shell"
      data-sidebar-hidden={!sidebar}
      data-settings-open={settings}
    >
      {children}
    </div>
  );
}
