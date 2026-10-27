import { ResizeHandle as Splitter } from "@swarmie/ui-web";
import { useRef } from "react";

import { useClient } from "../client/context";

export function ResizeHandle({ target }: { target: "navigation" | "panels" }) {
  const { shell } = useClient();

  const resize = useRef<{ x: number; value: number } | null>(null);
  function move(event: React.PointerEvent<HTMLDivElement>) {
    if (!resize.current) {
      return;
    }
    const dx =
      (event.clientX - resize.current.x) *
      (target === "panels" && !shell.getSnapshot().conversationFirst ? -1 : 1);
    const value =
      target === "navigation"
        ? Math.max(180, Math.min(340, resize.current.value + dx))
        : Math.max(
            28,
            Math.min(
              72,
              resize.current.value +
                (dx /
                  (document.querySelector("#client-panels")?.getBoundingClientRect().width ??
                    window.innerWidth)) *
                  100,
            ),
          );
    document.documentElement.style.setProperty(
      target === "navigation" ? "--navigation-width" : "--conversation-share",
      `${value}${target === "navigation" ? "px" : "%"}`,
    );
  }
  return (
    <Splitter
      aria-label={target === "navigation" ? "Resize navigation" : "Resize panels"}
      aria-orientation="vertical"
      onPointerDown={(event) => {
        const value = Number(
          getComputedStyle(document.documentElement)
            .getPropertyValue(
              target === "navigation" ? "--navigation-width" : "--conversation-share",
            )
            .replace(/(?:px|%)$/u, ""),
        );
        resize.current = {
          x: event.clientX,
          value: (() => {
            if (Number.isFinite(value)) {
              return value;
            }
            if (target === "navigation") {
              return 220;
            }
            return 50;
          })(),
        };
        event.currentTarget.setPointerCapture(event.pointerId);
        document.body.dataset.resizing = "true";
      }}
      onPointerMove={move}
      onPointerUp={() => {
        resize.current = null;
        delete document.body.dataset.resizing;
      }}
      onPointerCancel={() => {
        resize.current = null;
        delete document.body.dataset.resizing;
      }}
      onLostPointerCapture={() => {
        resize.current = null;
        delete document.body.dataset.resizing;
      }}
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
          return;
        }
        event.preventDefault();
        const key = target === "navigation" ? "--navigation-width" : "--conversation-share";
        const old = Number(
          getComputedStyle(document.documentElement)
            .getPropertyValue(key)
            .replace(/(?:px|%)$/u, ""),
        );
        document.documentElement.style.setProperty(
          key,
          `${Math.max(target === "navigation" ? 180 : 28, Math.min(target === "navigation" ? 340 : 72, old + (event.key === "ArrowRight" ? 1 : -1) * (target === "navigation" ? 12 : 3)))}${target === "navigation" ? "px" : "%"}`,
        );
      }}
      className="group relative z-20 hidden w-px shrink-0 cursor-col-resize md:block"
    >
      <span className="absolute inset-y-0 -left-1 w-2 group-hover:bg-brand-accent/40" />
    </Splitter>
  );
}
