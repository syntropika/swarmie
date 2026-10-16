import { Tooltip, Button, Dropdown, TooltipContent } from "@swarmie/ui-web";
import { ChevronDown, Check, ListFilter } from "lucide-react";
import type { ReactNode } from "react";

export type SidebarSort = "manual" | "updated_at";

export function SidebarSortMenu({
  label,
  value,
  onChange,
}: {
  label: string;
  value: SidebarSort;
  onChange: (value: SidebarSort) => void;
}) {
  return (
    <Dropdown>
      <Tooltip>
        <Button variant="tertiary" size="sm" isIconOnly aria-label={`${label} options`}>
          <ListFilter size={14} />
        </Button>
        <TooltipContent>{label} options</TooltipContent>
      </Tooltip>
      <Dropdown.Popover placement="bottom end">
        <Dropdown.Menu
          aria-label={`${label} sorting`}
          selectionMode="single"
          selectedKeys={new Set([value])}
          onAction={(key) => {
            if (key === "manual" || key === "updated_at") {
              onChange(key);
            }
          }}
        >
          <div className="px-2 py-1 text-xs text-muted">Sort by</div>
          <Dropdown.Item id="updated_at" textValue="Recent activity">
            Recent activity{value === "updated_at" && <Check size={14} className="ml-auto" />}
          </Dropdown.Item>
          <Dropdown.Item id="manual" textValue="Manual order">
            Manual order{value === "manual" && <Check size={14} className="ml-auto" />}
          </Dropdown.Item>
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>
  );
}

export function SidebarSection({
  label,
  collapsed,
  onToggle,
  actions,
  children,
}: {
  label: string;
  collapsed: boolean;
  onToggle: () => void;
  actions: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-label={label} className="group/nav-section mt-5">
      <div className="mb-1 flex h-7 items-center gap-1 px-2">
        <h2 className="min-w-0 flex-1">
          <Button
            variant="tertiary"
            size="sm"
            onClick={onToggle}
            aria-expanded={!collapsed}
            className="w-full justify-start"
          >
            <span className="truncate">{label}</span>
            <ChevronDown
              size={12}
              aria-hidden="true"
              className={`shrink-0 transition-transform ${collapsed ? "-rotate-90" : "opacity-0 group-focus-within/nav-section:opacity-100 group-hover/nav-section:opacity-100"}`}
            />
          </Button>
        </h2>
        <div className="pointer-events-none flex shrink-0 items-center gap-0.5 opacity-0 group-focus-within/nav-section:pointer-events-auto group-focus-within/nav-section:opacity-100 group-hover/nav-section:pointer-events-auto group-hover/nav-section:opacity-100">
          {actions}
        </div>
      </div>
      {!collapsed && children}
    </section>
  );
}
