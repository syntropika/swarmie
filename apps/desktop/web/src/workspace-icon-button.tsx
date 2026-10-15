import { Button } from "~/web/src/components/ui/button";
import { Tooltip, TooltipContent } from "~/web/src/components/ui/tooltip";
// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0

export function IconButton({
  label,
  children,
  onClick,
  active = false,
  disabled = false,
  sidebarToggle = false,
}: {
  label: string;
  children: React.ReactNode;
  onClick?: () => void;
  active?: boolean;
  disabled?: boolean;
  sidebarToggle?: boolean;
}) {
  return (
    <Tooltip delay={500}>
      <Button
        variant={active ? "secondary" : "ghost"}
        size="icon"
        aria-label={label}
        data-sidebar-toggle={sidebarToggle}
        onClick={onClick}
        disabled={disabled}
      >
        {children}
      </Button>
      <TooltipContent placement="bottom" offset={6}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
