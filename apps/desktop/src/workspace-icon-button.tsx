import { Button, Tooltip, TooltipContent } from "@swarmie/ui-web";
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
        variant={active ? "secondary" : "tertiary"}
        size="md"
        isIconOnly
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
