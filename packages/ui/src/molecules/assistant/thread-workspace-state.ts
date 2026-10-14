import { useEffect, useRef } from "react";
import type { KeyboardEvent, PointerEvent } from "react";

import {
  clampWorkspaceSize,
  DEFAULT_WORKSPACE_SIZE,
  parseWorkspaceSize,
} from "./thread-workspace-size";
import type { WorkspaceSize } from "./thread-workspace-size";

type ResizeDrag = {
  pointerId: number;
  x: number;
  y: number;
  width: number;
  height: number;
  direction: number;
};

/** Dynamic extent variables are neutral geometry; Tailwind continues to own all component styling. */
export function useFloatingWorkspace(
  open: boolean,
  onOpenChange: (open: boolean) => void,
  storageKey: string | undefined,
) {
  const panel = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const size = useRef(DEFAULT_WORKSPACE_SIZE);
  const drag = useRef<ResizeDrag | null>(null);
  const close = useRef(onOpenChange);
  useEffect(() => {
    close.current = onOpenChange;
  }, [onOpenChange]);

  const applySize = (next: WorkspaceSize) => {
    const value = clampWorkspaceSize(next, {
      width: window.innerWidth,
      height: window.innerHeight,
    });
    size.current = value;
    panel.current?.style.setProperty("--assistant-workspace-width", `${value.width}px`);
    panel.current?.style.setProperty("--assistant-workspace-height", `${value.height}px`);
    return value;
  };
  const persist = (value: WorkspaceSize) => {
    if (storageKey === undefined) {
      return;
    }
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(value));
    } catch {
      // Storage may be disabled; current-session resizing remains available.
    }
  };
  const commit = (next: WorkspaceSize) => {
    persist(applySize(next));
  };

  useEffect(() => {
    let stored: WorkspaceSize | undefined;
    if (storageKey !== undefined) {
      try {
        stored = parseWorkspaceSize(window.localStorage.getItem(storageKey));
      } catch {
        // Restricted storage must not prevent the assistant from opening.
      }
    }
    const update = () => {
      const value = clampWorkspaceSize(size.current, {
        width: window.innerWidth,
        height: window.innerHeight,
      });
      size.current = value;
      panel.current?.style.setProperty("--assistant-workspace-width", `${value.width}px`);
      panel.current?.style.setProperty("--assistant-workspace-height", `${value.height}px`);
    };
    size.current = stored ?? DEFAULT_WORKSPACE_SIZE;
    update();
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("resize", update);
      drag.current = null;
    };
  }, [storageKey]);

  useEffect(() => {
    const element = panel.current;
    if (element === null) {
      return undefined;
    }
    drag.current = null;
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    let current = true;
    const restoreFocus = element.contains(document.activeElement);
    const finish = () => {
      if (!current) {
        return;
      }
      if (!open) {
        element.close();
        if (restoreFocus) {
          trigger.current?.focus();
        }
      }
    };
    if (open && !element.open) {
      element.show();
      element.querySelector<HTMLTextAreaElement>("textarea:not(:disabled)")?.focus();
    }
    if (element.open && !preference.matches) {
      animation = element.animate(
        open
          ? [
              { opacity: 0, transform: "translateY(8px) scale(0.95)" },
              { opacity: 1, transform: "translateY(0) scale(1)" },
            ]
          : [
              { opacity: 1, transform: "translateY(0) scale(1)" },
              { opacity: 0, transform: "translateY(8px) scale(0.95)" },
            ],
        { duration: open ? 300 : 200, easing: "cubic-bezier(0.32,0.72,0,1)", fill: "both" },
      );
      animation.onfinish = finish;
    } else {
      finish();
    }
    const reduce = () => {
      if (preference.matches) {
        animation?.cancel();
        finish();
      }
    };
    const escape = (event: globalThis.KeyboardEvent) => {
      if (document.querySelector(":modal") !== null) {
        return;
      }
      if (open && event.key === "Escape" && element.contains(document.activeElement)) {
        event.preventDefault();
        close.current(false);
      }
    };
    preference.addEventListener("change", reduce);
    document.addEventListener("keydown", escape);
    return () => {
      current = false;
      animation?.cancel();
      preference.removeEventListener("change", reduce);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  useEffect(() => {
    const element = trigger.current;
    if (element === null) {
      return undefined;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (preference.matches) {
      return undefined;
    }
    const animation = element.animate(
      [
        {
          opacity: 0.6,
          transform: open ? "rotate(-20deg) scale(0.9)" : "rotate(20deg) scale(0.9)",
        },
        { opacity: 1, transform: "rotate(0) scale(1)" },
      ],
      { duration: 200, easing: "cubic-bezier(0.2,0,0,1)" },
    );
    const cancel = () => {
      animation.cancel();
    };
    preference.addEventListener("change", cancel);
    return () => {
      cancel();
      preference.removeEventListener("change", cancel);
    };
  }, [open]);

  const reset = () => {
    drag.current = null;
    commit(DEFAULT_WORKSPACE_SIZE);
  };
  const direction = () =>
    panel.current !== null && getComputedStyle(panel.current).direction === "rtl" ? -1 : 1;
  const fromPointer = (event: PointerEvent<HTMLButtonElement>, active: ResizeDrag) => ({
    width: active.width - (event.clientX - active.x) * active.direction,
    height: active.height - (event.clientY - active.y),
  });
  return {
    panel,
    trigger,
    handleProps: {
      onDoubleClick: reset,
      onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
        if (event.button !== 0 || panel.current === null) {
          return;
        }
        const rect = panel.current.getBoundingClientRect();
        drag.current = {
          pointerId: event.pointerId,
          x: event.clientX,
          y: event.clientY,
          width: rect.width,
          height: rect.height,
          direction: direction(),
        };
        event.currentTarget.setPointerCapture(event.pointerId);
        event.preventDefault();
      },
      onPointerMove: (event: PointerEvent<HTMLButtonElement>) => {
        const active = drag.current;
        if (active?.pointerId !== event.pointerId) {
          return;
        }
        applySize(fromPointer(event, active));
      },
      onPointerUp: (event: PointerEvent<HTMLButtonElement>) => {
        const active = drag.current;
        if (active?.pointerId !== event.pointerId) {
          return;
        }
        drag.current = null;
        commit(fromPointer(event, active));
        event.currentTarget.releasePointerCapture(event.pointerId);
      },
      onPointerCancel: () => {
        drag.current = null;
        persist(size.current);
      },
      onLostPointerCapture: () => {
        drag.current = null;
      },
      onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          reset();
          return;
        }
        const step = event.shiftKey ? 64 : 16;
        let width = 0;
        let height = 0;
        switch (event.key) {
          case "ArrowLeft": {
            width = step * direction();
            break;
          }
          case "ArrowRight": {
            width = -step * direction();
            break;
          }
          case "ArrowUp": {
            height = step;
            break;
          }
          case "ArrowDown": {
            height = -step;
            break;
          }
          default: {
            return;
          }
        }
        event.preventDefault();
        commit({ width: size.current.width + width, height: size.current.height + height });
      },
    },
  };
}
