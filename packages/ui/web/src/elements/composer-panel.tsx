import type { ComponentProps } from "react";

import { cn } from "../classes";

/** A conversation composer owns one boundary around context, input and supplied controls. */
function Root({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      data-slot="composer-panel"
      className={cn("relative w-full min-w-0 text-foreground", className)}
    />
  );
}

function Context({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      className={cn(
        "-mb-px flex min-h-[34px] min-w-0 items-center gap-1.5 rounded-t-menu border border-b-0 border-border bg-composer-context px-2 py-0.5 text-chrome text-muted",
        className,
      )}
    />
  );
}

function Body({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      className={cn(
        "relative min-w-0 rounded-b-composer border border-composer-border bg-composer px-(--spacing-composer-x) pt-(--spacing-composer-top) pb-(--spacing-composer-bottom)",
        className,
      )}
    />
  );
}

export const ComposerPanel = Object.assign(Root, { Root, Context, Body });
