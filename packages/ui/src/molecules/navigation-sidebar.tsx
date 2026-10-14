import type { ReactNode } from "react";

export type NavigationSidebarProps = {
  label: string;
  header: ReactNode;
  children: ReactNode;
};

/** The contextual column keeps identical spacing while capabilities replace its content. */
export function NavigationSidebar({ label, header, children }: NavigationSidebarProps) {
  return (
    <aside
      aria-label={label}
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col rounded-l-marketing-card bg-canvas px-2 pb-2"
    >
      <header className="flex min-h-12 min-w-0 shrink-0 items-center gap-1">{header}</header>
      <div className="mt-1 flex min-h-0 flex-1 flex-col">{children}</div>
    </aside>
  );
}
