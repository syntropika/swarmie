import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../atoms/button";
import { Icon } from "../atoms/icon";
import {
  NAVIGATION_DEFAULT_WIDTH,
  NAVIGATION_RAIL_WIDTH,
  NAVIGATION_WIDTH_STORAGE_KEY,
  navigationLayout,
  readNavigationWidth,
} from "./navigation-layout";
import type { NavigationLayout } from "./navigation-layout";
import { NavigationResizeHandle } from "./navigation-resize-handle";
import { useNavigationPreview } from "./use-navigation-preview";

function persistNavigationWidth(width: number): boolean {
  try {
    window.sessionStorage.setItem(NAVIGATION_WIDTH_STORAGE_KEY, String(width));
    return true;
  } catch {
    // A blocked browser store leaves the mounted shell's chosen width in memory.
    return false;
  }
}

export type NavigationShellRailControls = {
  closeNavigation: () => void;
  revealSidebar: () => void;
  previewNavigation: (options?: { focus?: boolean }) => void;
  leavePreview: () => void;
};

export type NavigationShellSidebarControls = {
  navigationControl: ReactNode;
  closeNavigation: () => void;
  revealSidebar: () => void;
};

export type NavigationShellProps = {
  labels: {
    navigation: string;
    openNavigation: string;
    closeNavigation: string;
    collapseNavigation: string;
    pinNavigation: string;
    resizeNavigation: string;
  };
  renderRail: (controls: NavigationShellRailControls) => ReactNode;
  renderSidebar: (controls: NavigationShellSidebarControls) => ReactNode;
  collapsible?: boolean;
  children: ReactNode;
};

/** Capabilities supply navigation content; this shell owns responsive geometry and focus. */
export function NavigationShell({
  labels,
  renderRail,
  renderSidebar,
  collapsible = true,
  children,
}: NavigationShellProps) {
  const [layout, setLayout] = useState<NavigationLayout>({
    mode: "pinned",
    width: NAVIGATION_DEFAULT_WIDTH,
  });
  const [shell, setShell] = useState<HTMLDivElement | null>(null);
  const [mobileDialog, setMobileDialog] = useState<HTMLDialogElement | null>(null);
  const [temporaryDialog, setTemporaryDialog] = useState<HTMLDialogElement | null>(null);
  const previewReturnFocus = useRef<HTMLElement | null>(null);
  const [focusPreview, setFocusPreview] = useState(false);
  const [widthRestored, setWidthRestored] = useState(false);
  const desktopId = useId();
  const temporaryId = useId();
  const mobileId = useId();

  const dismissPreview = useCallback(() => {
    setLayout((current) => navigationLayout(current, { type: "dismiss" }));
  }, []);
  const requestPreview = useCallback(
    (focus: boolean) => {
      if (collapsible && window.matchMedia("(min-width: 768px)").matches) {
        setFocusPreview(focus);
        setLayout((current) => navigationLayout(current, { type: "preview" }));
      }
    },
    [collapsible],
  );
  const preview = useNavigationPreview({
    dialog: temporaryDialog,
    onPreview: requestPreview,
    onDismiss: dismissPreview,
  });
  const { cancel: cancelPreview } = preview;
  const closeNavigation = useCallback(() => {
    mobileDialog?.close();
    cancelPreview();
    dismissPreview();
  }, [mobileDialog, cancelPreview, dismissPreview]);
  const revealSidebar = useCallback(() => {
    cancelPreview();
    setLayout((current) => navigationLayout(current, { type: "pin" }));
    mobileDialog?.close();
  }, [mobileDialog, cancelPreview]);
  const closeSidebar = () => {
    setLayout((current) => navigationLayout(current, { type: "close" }));
    shell?.querySelector<HTMLElement>('[data-navigation-home="true"]')?.focus();
  };
  const resize = (totalWidth: number) => {
    setLayout((current) => navigationLayout(current, { type: "resize", totalWidth, collapsible }));
  };
  const resizeOrigin = () =>
    (shell?.getBoundingClientRect().left ?? 0) + (layout.mode === "temporary" ? 4 : 0);

  useLayoutEffect(() => {
    let width = NAVIGATION_DEFAULT_WIDTH;
    try {
      width = readNavigationWidth(window.sessionStorage.getItem(NAVIGATION_WIDTH_STORAGE_KEY));
    } catch {
      // Blocked browser storage leaves this mounted shell's width in memory.
      width = NAVIGATION_DEFAULT_WIDTH;
    }
    // oxlint-disable-next-line react/set-state-in-effect -- Synchronize external session storage after the matching server/client initial layout.
    setLayout((current) => navigationLayout(current, { type: "restore-width", width }));
    setWidthRestored(true);
  }, []);

  useLayoutEffect(() => {
    // This measured dimension is layout geometry only; all appearance remains in Tailwind.
    shell?.style.setProperty("--navigation-width", `${layout.width}px`);
  }, [shell, layout.width]);

  useEffect(() => {
    if (!widthRestored) {
      return;
    }
    persistNavigationWidth(layout.width);
  }, [layout.width, widthRestored]);

  useLayoutEffect(() => {
    if (temporaryDialog === null) {
      return;
    }
    if (layout.mode === "temporary" && collapsible && !temporaryDialog.matches(":popover-open")) {
      const previousFocus = document.activeElement;
      previewReturnFocus.current = previousFocus instanceof HTMLElement ? previousFocus : null;
      temporaryDialog.showPopover();
      if (!focusPreview && previousFocus instanceof HTMLElement) {
        previousFocus.focus({ preventScroll: true });
        if (
          document.activeElement !== previousFocus &&
          document.activeElement instanceof HTMLElement
        ) {
          document.activeElement.blur();
        }
      }
    } else if (
      layout.mode === "temporary" &&
      focusPreview &&
      temporaryDialog.matches(":popover-open")
    ) {
      temporaryDialog.querySelector<HTMLElement>("button:not([disabled])")?.focus();
    } else if (layout.mode !== "temporary" && temporaryDialog.matches(":popover-open")) {
      temporaryDialog.hidePopover();
    }
  }, [layout.mode, collapsible, temporaryDialog, focusPreview]);

  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 768px)");
    const dismissAtBreakpoint = () => {
      if (desktop.matches) {
        mobileDialog?.close();
      } else {
        setLayout((current) => navigationLayout(current, { type: "dismiss" }));
      }
    };
    desktop.addEventListener("change", dismissAtBreakpoint);
    return () => {
      desktop.removeEventListener("change", dismissAtBreakpoint);
    };
  }, [mobileDialog]);

  const railControls = {
    closeNavigation,
    revealSidebar,
    previewNavigation: preview.enter,
    leavePreview: preview.leave,
  };
  const resizeHandle = (
    <NavigationResizeHandle
      label={labels.resizeNavigation}
      totalWidth={NAVIGATION_RAIL_WIDTH + layout.width}
      origin={resizeOrigin}
      onResize={resize}
    />
  );

  return (
    <div ref={setShell} className="relative flex h-dvh min-h-0 bg-canvas text-ink">
      <div className="hidden h-dvh min-h-0 shrink-0 bg-rail md:flex">
        {renderRail(railControls)}
        {layout.mode === "pinned" || !collapsible ? (
          <div className="relative h-full w-[var(--navigation-width,288px)] shrink-0">
            <div
              id={desktopId}
              className="h-full overflow-hidden rounded-l-marketing-card border border-transparent bg-canvas"
            >
              {renderSidebar({
                closeNavigation,
                revealSidebar,
                navigationControl: collapsible ? (
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={closeSidebar}
                    aria-label={labels.collapseNavigation}
                    title={labels.collapseNavigation}
                  >
                    <Icon name="sidebar" />
                  </Button>
                ) : null,
              })}
            </div>
            {resizeHandle}
          </div>
        ) : null}
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="absolute top-3 left-4 z-20 md:hidden">
          <Button
            variant="ghost"
            size="icon"
            aria-label={labels.openNavigation}
            title={labels.openNavigation}
            aria-haspopup="dialog"
            aria-controls={mobileId}
            onClick={() => mobileDialog?.showModal()}
          >
            <Icon name="sidebar" />
          </Button>
        </div>
        {children}
      </div>
      <dialog
        ref={setTemporaryDialog}
        id={temporaryId}
        popover="auto"
        aria-label={labels.navigation}
        className="fixed inset-y-1 left-[56px] m-0 h-auto w-[var(--navigation-width,288px)] overflow-visible rounded-l-marketing-card rounded-r-panel border-0 bg-canvas p-0 text-ink shadow-overlay"
        onPointerEnter={preview.retain}
        onPointerLeave={preview.leave}
        onBlurCapture={(event) => {
          preview.focusLeave(event.relatedTarget);
        }}
        onKeyDown={(event) => {
          if (
            event.key !== "Escape" ||
            !(event.target instanceof Element) ||
            event.target.closest("[popover]:popover-open") !== event.currentTarget
          ) {
            return;
          }
          event.preventDefault();
          event.stopPropagation();
          closeNavigation();
          shell?.querySelector<HTMLElement>('[data-navigation-home="true"]')?.focus();
        }}
        onToggle={(event) => {
          event.stopPropagation();
          if (event.newState === "closed") {
            cancelPreview();
            setLayout((current) => navigationLayout(current, { type: "dismiss" }));
            if (
              event.currentTarget.contains(document.activeElement) ||
              document.activeElement === document.body
            ) {
              previewReturnFocus.current?.focus({ preventScroll: true });
            }
          }
        }}
      >
        <div className="h-full overflow-hidden rounded-l-marketing-card rounded-r-panel">
          {renderSidebar({
            closeNavigation,
            revealSidebar,
            navigationControl: (
              <Button
                variant="ghost"
                size="icon"
                onClick={revealSidebar}
                aria-label={labels.pinNavigation}
                title={labels.pinNavigation}
              >
                <Icon name="pin" />
              </Button>
            ),
          })}
        </div>
        {resizeHandle}
      </dialog>
      <dialog
        ref={setMobileDialog}
        id={mobileId}
        aria-label={labels.navigation}
        className="fixed inset-y-0 left-0 m-0 h-dvh max-h-none w-[min(360px,calc(100vw-16px))] max-w-none overflow-hidden rounded-r-marketing-card border-0 bg-rail p-0 text-ink shadow-overlay backdrop:bg-inverse/40"
      >
        <div className="flex h-full min-h-0">
          {renderRail(railControls)}
          {renderSidebar({
            closeNavigation,
            revealSidebar,
            navigationControl: (
              <Button
                variant="ghost"
                size="icon"
                onClick={closeNavigation}
                aria-label={labels.closeNavigation}
                title={labels.closeNavigation}
              >
                <Icon name="close" />
              </Button>
            ),
          })}
        </div>
      </dialog>
    </div>
  );
}
