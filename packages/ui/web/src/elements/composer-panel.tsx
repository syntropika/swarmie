import type { ComponentProps } from "react";

import { cn } from "../classes";

/** A conversation composer owns one boundary around context, input and supplied controls. */
function Root({ className, children, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      data-slot="composer-panel"
      className={cn(
        "group/composer relative isolate grid w-full min-w-0 grid-cols-1 grid-rows-[auto_auto] px-(--spacing-composer-inset) text-foreground",
        className,
      )}
    >
      <div
        aria-hidden="true"
        data-slot="composer-surface"
        className="pointer-events-none z-10 col-start-1 row-start-2 -mx-(--spacing-composer-inset) rounded-composer border border-composer-border bg-composer shadow-none"
      />
      {children}
    </div>
  );
}

function Context({ className, children, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      data-slot="composer-context"
      className={cn(
        "relative col-start-1 row-start-1 min-w-0 text-chrome text-foreground",
        className,
      )}
    >
      <div
        aria-hidden="true"
        data-slot="composer-context-surface"
        className="pointer-events-none absolute inset-x-0 top-0 h-[200%] rounded-t-composer-context bg-composer-context"
      />
      <div
        data-slot="composer-context-controls"
        className="relative flex min-h-(--spacing-chrome-menu-row) min-w-0 flex-wrap items-center justify-between gap-(--spacing-control-group) px-(--spacing-composer-context-x) py-(--spacing-composer-context-y)"
      >
        {children}
      </div>
    </div>
  );
}

function Body({ className, ...props }: ComponentProps<"div">) {
  return (
    <div
      {...props}
      className={cn(
        "relative z-10 col-start-1 row-start-2 min-w-0 pb-(--spacing-composer-body-bottom)",
        className,
      )}
    />
  );
}

export const ComposerPanel = Object.assign(Root, { Root, Context, Body });
