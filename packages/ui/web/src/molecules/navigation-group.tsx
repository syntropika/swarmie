import type { ReactNode } from "react";

import { Disclosure } from "../atoms/disclosure";
import { Icon } from "../atoms/icon";
import { cn } from "../classes";

/** Compact sidebar disclosure keeps plugin-owned header actions outside the toggle. */
export function NavigationGroup({
  title,
  expanded,
  onExpandedChange,
  actions,
  children,
  className,
}: {
  readonly title: string;
  readonly expanded: boolean;
  readonly onExpandedChange: (expanded: boolean) => void;
  readonly actions?: ReactNode;
  readonly children: ReactNode;
  readonly className?: string;
}) {
  return (
    <section aria-label={title} className={cn("min-w-0", className)}>
      <Disclosure isExpanded={expanded} onExpandedChange={onExpandedChange}>
        <div className="flex min-w-0 items-center gap-0.5">
          <Disclosure.Heading level={2} className="min-w-0 flex-1">
            <Disclosure.Trigger className="flex min-h-(--spacing-navigation-group) w-full min-w-0 items-center gap-1 rounded-small px-(--spacing-navigation-row-x) text-left text-navigation-heading font-normal text-muted hover:text-foreground">
              <span className="truncate">{title}</span>
              <Icon name="chevron" className={cn("size-3 shrink-0", !expanded && "-rotate-90")} />
            </Disclosure.Trigger>
          </Disclosure.Heading>
          {actions}
        </div>
        <Disclosure.Content>
          <Disclosure.Body className="grid gap-0 p-0">{children}</Disclosure.Body>
        </Disclosure.Content>
      </Disclosure>
    </section>
  );
}
