import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";

export type NativeSelectProps = ComponentPropsWithRef<"select"> & {
  tone?: "surface" | "quiet";
  controlSize?: "default" | "compact";
};

/** Preserve the platform picker, form semantics, keyboard behavior, and native ref. */
export function NativeSelect({
  className,
  tone = "surface",
  controlSize = "default",
  ...props
}: NativeSelectProps) {
  const isListbox = props.multiple === true || (props.size ?? 0) > 1;
  return (
    <span className="relative block w-full min-w-0">
      <select
        {...props}
        className={cn(
          "peer min-h-control w-full min-w-0 appearance-none border border-transparent px-3 py-1.5 pr-8 text-ui leading-ui font-medium text-ink transition-colors duration-150 ease-swarmie focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-danger aria-invalid:focus-visible:outline-danger motion-reduce:transition-none pointer-coarse:min-h-control-touch",
          "rounded-pill",
          tone === "quiet"
            ? "bg-transparent enabled:hover:bg-surface-hover"
            : "bg-surface-muted enabled:hover:bg-surface-hover",
          controlSize === "compact" && "min-h-control-compact py-0 pr-7 pl-2",
          className,
        )}
      />
      {isListbox ? null : (
        <Icon
          name="chevron"
          className={cn(
            "pointer-events-none absolute top-1/2 size-3.5 -translate-y-1/2 text-ink peer-disabled:opacity-50",
            controlSize === "compact" ? "right-2" : "right-3",
          )}
        />
      )}
    </span>
  );
}
