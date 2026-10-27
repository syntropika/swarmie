import { ArrowLeft, ArrowRight } from "lucide-react";
import { useState } from "react";
import type { DragEvent } from "react";

import { iconStyle } from "../controls";

/** Shows the destination of a whole-panel move without shifting the existing content. */
export function PanelDropOverlay({
  panel,
  onDrop,
}: {
  panel: "conversation" | "view";
  onDrop: (side: "left" | "right", event: DragEvent) => void;
}) {
  const [target, setTarget] = useState<"left" | "right" | null>(null);
  return (
    <div className="pointer-events-none absolute inset-0 z-40 hidden md:flex">
      {(["left", "right"] as const).map((side) => {
        const Arrow = side === "left" ? ArrowLeft : ArrowRight;
        return (
          <div
            key={side}
            onDragOver={(event) => {
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
              setTarget(side);
            }}
            onDragLeave={() => {
              setTarget(null);
            }}
            onDrop={(event) => {
              event.preventDefault();
              onDrop(side, event);
            }}
            className={`pointer-events-auto flex w-1/2 items-center justify-center border ${target === side ? "border-panel-drop-line bg-panel-drop-surface" : "border-transparent"}`}
          >
            {target === side ? (
              <output className="pointer-events-none flex items-center gap-2 rounded-lg bg-panel-drop-label px-4 py-3 text-sm text-panel-drop-foreground shadow-lg">
                <Arrow className={iconStyle} />
                Move {panel === "conversation" ? "conversation" : "view"} to the {side}
              </output>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
