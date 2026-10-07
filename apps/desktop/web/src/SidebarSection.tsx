import type { ReactNode } from "react";
import { ChevronDown, Check, ListFilter } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { TooltipContent } from "@/components/ui/tooltip";

export type SidebarSort = "manual" | "updated_at";

export function SidebarSortMenu({ label, value, onChange }: {
  label: string; value: SidebarSort; onChange: (value: SidebarSort) => void;
}) {
  return <Dropdown triggerTooltip={<TooltipContent>{label} options</TooltipContent>}>
    <Button variant="ghost" size="icon-sm" aria-label={`${label} options`} className="text-muted"><ListFilter size={14} /></Button>
    <Dropdown.Popover placement="bottom end"><Dropdown.Menu aria-label={`${label} sorting`} selectionMode="single" selectedKeys={new Set([value])} onAction={(key) => onChange(key as SidebarSort)}>
      <div className="px-2 py-1 text-xs text-muted">Sort by</div>
      <Dropdown.Item id="updated_at" textValue="Recent activity">Recent activity{value === "updated_at" && <Check size={14} className="ml-auto" />}</Dropdown.Item>
      <Dropdown.Item id="manual" textValue="Manual order">Manual order{value === "manual" && <Check size={14} className="ml-auto" />}</Dropdown.Item>
    </Dropdown.Menu></Dropdown.Popover>
  </Dropdown>;
}

export function SidebarSection({ label, collapsed, onToggle, actions, children }: {
  label: string; collapsed: boolean; onToggle: () => void; actions: ReactNode; children: ReactNode;
}) {
  return <section aria-label={label} className="group/nav-section mt-5">
    <div className="mb-1 flex h-7 items-center gap-1 px-2">
      <h2 className="min-w-0 flex-1">
        <Button variant="ghost" size="sm" onClick={onToggle} aria-expanded={!collapsed} className="w-full justify-start gap-1 px-0 text-xs font-medium text-muted hover:bg-transparent">
          <span className="truncate">{label}</span>
          <ChevronDown size={12} aria-hidden="true" className={`shrink-0 transition-transform ${collapsed ? "-rotate-90" : "opacity-0 group-hover/nav-section:opacity-100 group-focus-within/nav-section:opacity-100"}`} />
        </Button>
      </h2>
      <div className="pointer-events-none flex shrink-0 items-center gap-0.5 opacity-0 group-hover/nav-section:pointer-events-auto group-hover/nav-section:opacity-100 group-focus-within/nav-section:pointer-events-auto group-focus-within/nav-section:opacity-100">{actions}</div>
    </div>
    {!collapsed && children}
  </section>;
}
