import { useSource } from "@swarmie/client-ui";
import { PaneResize } from "@swarmie/ui-web";
import { useEffect, useLayoutEffect, useState } from "react";

import { useClient } from "../client/context";

/** Clear pointer feedback when resizing finishes or its control is removed. */
function finishResize() {
  delete document.body.dataset.resizing;
}

/** Adapts the shared pixel splitter to the shell's persistent navigation width and panel ratio. */
export function ResizeHandle({ target }: { target: "navigation" | "panels" }) {
  const { shell } = useClient();
  const layout = useSource(shell);
  const navigation = target === "navigation";
  const property = navigation ? "--navigation-width" : "--conversation-share";
  const [value, setValue] = useState(() => {
    const stored = getComputedStyle(document.documentElement).getPropertyValue(property).trim();
    const saved = Number(stored.replace(/(?:px|%)$/u, ""));
    if (stored !== "" && Number.isFinite(saved)) {
      return saved;
    }
    return navigation ? 220 : 50;
  });
  const [extent, setExtent] = useState(1);

  useLayoutEffect(() => {
    if (navigation) {
      return undefined;
    }
    const panels = document.querySelector("#client-panels");
    if (!panels) {
      return undefined;
    }
    const measure = () => {
      setExtent(Math.max(1, panels.getBoundingClientRect().width));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panels);
    return () => {
      observer.disconnect();
    };
  }, [navigation]);

  useEffect(() => finishResize, []);

  return (
    <div
      className={`relative hidden w-px shrink-0 md:block ${navigation ? "" : "bg-border"}`}
      onPointerDownCapture={(event) => {
        if (event.button === 0) {
          document.body.dataset.resizing = "true";
        }
      }}
      onPointerUp={finishResize}
      onPointerCancel={finishResize}
      onLostPointerCapture={finishResize}
    >
      <PaneResize
        className="absolute inset-y-0 -left-1 z-20 w-[9px] cursor-col-resize"
        label={navigation ? "Resize navigation" : "Resize panels"}
        controls={navigation ? "client-navigation" : "client-panels"}
        width={navigation ? value : (value / 100) * extent}
        minimum={navigation ? 180 : extent * 0.28}
        maximum={navigation ? 340 : extent * 0.72}
        reset={navigation ? 220 : extent * 0.5}
        edge={navigation || layout.conversationFirst ? "right" : "left"}
        onResize={(pixels) => {
          const next = navigation ? pixels : (pixels / extent) * 100;
          setValue(next);
          document.documentElement.style.setProperty(property, `${next}${navigation ? "px" : "%"}`);
        }}
      />
    </div>
  );
}
