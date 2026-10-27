import { NavigationGroup as SharedNavigationGroup } from "@swarmie/ui-web";
import type { ReactNode } from "react";

export function NavigationGroup({
  title,
  folded,
  onToggle,
  children,
}: {
  title: string;
  folded: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <div className="mb-5">
      <SharedNavigationGroup title={title} expanded={!folded} onExpandedChange={onToggle}>
        {children}
      </SharedNavigationGroup>
    </div>
  );
}
