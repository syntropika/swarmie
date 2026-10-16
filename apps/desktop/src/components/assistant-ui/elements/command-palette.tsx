// Adapted from assistant-ui Elements; see LICENSE in this directory.
"use client";

import { Command } from "cmdk";
import { cn } from "cn";
import { SearchIcon } from "lucide-react";
import { useEffect, useId, useCallback } from "react";
import type { ComponentProps } from "react";

import { field, floating, mono } from "./surfaces";

export type PaletteCommand = {
  id: string;
  label: string;
  group: string;
  keys: readonly string[];
};

export function CommandPalette({
  commands,
  query,
  activeId,
  onQueryChange,
  onActiveChange,
  onRun,
  className,
  ...props
}: Omit<
  ComponentProps<typeof Command>,
  "children" | "commands" | "query" | "activeId" | "onQueryChange" | "onActiveChange" | "onRun"
> & {
  commands: readonly PaletteCommand[];
  query: string;
  activeId: string;
  onQueryChange?: (query: string) => void;
  onActiveChange?: (id: string) => void;
  onRun?: (id: string) => void;
}) {
  const listId = useId();
  const optionId = useCallback((id: string) => `${listId}-${id}`, [listId]);
  const matches = commands.filter((command) =>
    command.label.toLowerCase().includes(query.toLowerCase()),
  );
  const groups = [...new Set(matches.map((command) => command.group))];
  // display order, which is grouped and so differs from the filter order
  const ordered = groups.flatMap((group) => matches.filter((command) => command.group === group));

  const move = (delta: number) => {
    if (ordered.length === 0) {
      return;
    }
    const at = ordered.findIndex((command) => command.id === activeId);
    // activeId can be filtered out by the query; start from the edge the key implies
    let from = at;
    if (at === -1) {
      from = delta > 0 ? -1 : 0;
    }
    const next = ordered[(from + delta + ordered.length) % ordered.length];
    if (onActiveChange !== undefined) {
      onActiveChange(next.id);
    }
  };

  // aria-activedescendant moves the highlight without moving focus, and only
  // focus scrolls; the list is short enough to walk the active row out of view.
  useEffect(() => {
    document
      .querySelector(`[id="${CSS.escape(optionId(activeId))}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeId, optionId]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) {
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      event.stopPropagation();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      move(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      event.stopPropagation();
      const active = ordered.find((command) => command.id === activeId);
      if (active && onRun) {
        onRun(active.id);
      }
    }
  };

  return (
    <Command
      shouldFilter={false}
      value={activeId}
      onValueChange={onActiveChange}
      data-slot="command-palette"
      className={cn(
        floating,
        "flex w-full max-w-sm flex-col overflow-hidden rounded-2xl",
        className,
      )}

      {...props}
    >
      <div className="flex items-center gap-2.5 px-3.5 py-3">
        <SearchIcon className="size-3.5 shrink-0 text-muted" />
        <input
          value={query}
          onChange={(event) => onQueryChange?.(event.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Type a command"
          aria-label="Type a command"
          role="combobox"
          aria-expanded={ordered.length > 0}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={
            ordered.some((command) => command.id === activeId) ? optionId(activeId) : undefined
          }
          className="min-w-0 flex-1 bg-transparent text-sm text-foreground/85 outline-none placeholder:text-muted"
        />
        <span className={cn(field, mono, "rounded px-1.5 py-0.5 text-muted")}>esc</span>
      </div>

      <Command.List
        id={listId}
        label="Commands"
        className="flex max-h-72 flex-col overflow-y-auto border-t border-foreground/[0.07] p-1.5"
      >
        {groups.map((group) => (
          <Command.Group key={group} aria-label={group} className="flex flex-col">
            <span aria-hidden className={cn(mono, "px-2 pt-2 pb-1 text-muted")}>
              {group}
            </span>
            {matches
              .filter((command) => command.group === group)
              .map((command) => {
                const rowClasses = cn(
                  "flex items-center gap-2 rounded-xl px-2 py-1.5 text-start transition-colors",
                  command.id === activeId && "bg-foreground/[0.06]",
                  command.id !== activeId && onRun !== undefined && "hover:bg-foreground/[0.03]",
                );
                const content = (
                  <>
                    <span className="min-w-0 flex-1 truncate text-sm">{command.label}</span>
                    <span className="flex shrink-0 gap-1">
                      {command.keys.map((key) => (
                        <span
                          key={key}
                          className={cn(field, mono, "rounded px-1.5 py-0.5 text-muted")}
                        >
                          {key}
                        </span>
                      ))}
                    </span>
                  </>
                );

                return (
                  <Command.Item
                    key={command.id}
                    id={optionId(command.id)}
                    value={command.id}
                    aria-selected={command.id === activeId}
                    onMouseDown={(event) => {
                      event.preventDefault();
                    }}
                    onSelect={() => {
                      onRun?.(command.id);
                    }}
                    className={rowClasses}
                  >
                    {content}
                  </Command.Item>
                );
              })}
          </Command.Group>
        ))}
      </Command.List>
      {matches.length === 0 && (
        <div className="border-t border-foreground/[0.07] p-1.5">
          <span className="block px-2 py-4 text-center text-xs break-words text-muted">
            No command matches “{query}”
          </span>
        </div>
      )}
    </Command>
  );
}
