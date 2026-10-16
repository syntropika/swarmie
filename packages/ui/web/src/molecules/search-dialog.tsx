import { Command } from "cmdk";
import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import { Button } from "../atoms/button";
import { CloseButton } from "../atoms/close-button";
import { Icon } from "../atoms/icon";
import { Modal } from "../atoms/modal";

export type SearchDialogItem = {
  id: string;
  label: string;
  description?: string;
  icon?: ReactNode;
  metadata?: ReactNode;
};

export type SearchDialogSection = {
  id: string;
  label: string;
  items: readonly SearchDialogItem[];
};

export type SearchDialogProps = {
  title: string;
  inputLabel?: string;
  placeholder: string;
  query: string;
  onQueryChange: (query: string) => void;
  /** Results are externally filtered, including remote or full-text matches. */
  sections: readonly SearchDialogSection[];
  onAction: (id: string) => void;
  onClose: () => void;
  /** Scoped searches can return to the complete collection without dismissing the modal. */
  context?: { label: string; onBack: () => void } | undefined;
  emptyMessage: string;
  emptyAction?: { label: string; onPress: () => void };
  status?: ReactNode;
};

/** HeroUI owns the overlay; cmdk owns the command collection and input navigation. */
export function SearchDialog({
  title,
  inputLabel = title,
  placeholder,
  query,
  onQueryChange,
  sections,
  onAction,
  onClose,
  context,
  emptyMessage,
  emptyAction,
  status,
}: SearchDialogProps) {
  const [selection, setSelection] = useState({ context: context?.label, id: "" });
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const visibleSections = sections.filter((section) => section.items.length > 0);
  const selected =
    selection.context === context?.label ? selection.id : (visibleSections[0]?.items[0]?.id ?? "");
  // Controlled group jumps need the same scrolling that cmdk provides for arrow navigation.
  useEffect(() => {
    list.current
      ?.querySelector(`[cmdk-item][data-value="${CSS.escape(selected)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  const onInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing || event.altKey || event.ctrlKey || event.metaKey) {
      return;
    }
    if (event.key === "Backspace" && query.length === 0 && context !== undefined) {
      event.preventDefault();
      event.stopPropagation();
      context.onBack();
    }
    if (event.key === "Tab" && visibleSections.length > 1) {
      event.preventDefault();
      event.stopPropagation();
      const index = visibleSections.findIndex((section) =>
        section.items.some((item) => item.id === selected),
      );
      const next =
        visibleSections[
          (index + (event.shiftKey ? -1 : 1) + visibleSections.length) % visibleSections.length
        ];
      const item = next?.items[0];
      if (item !== undefined) {
        setSelection({ context: context?.label, id: item.id });
      }
    }
  };
  return (
    <Modal.Backdrop
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <Modal.Container className="search-dialog__container" placement="top" scroll="inside">
        <Modal.Dialog className="search-dialog__surface" aria-label={title}>
          <Modal.Heading className="sr-only">{title}</Modal.Heading>
          {context === undefined ? null : (
            <div className="search-dialog__navigation">
              <Button
                variant="tertiary"
                size="sm"
                onPress={() => {
                  context.onBack();
                  input.current?.focus();
                }}
              >
                <Icon name="back" className="size-4" />
                All results
              </Button>
              <span className="text-xs text-muted">{context.label}</span>
            </div>
          )}
          <Command
            className="search-dialog__command"
            label={inputLabel}
            shouldFilter={false}
            vimBindings={false}
            loop
            value={selected}
            onValueChange={(id) => {
              setSelection({ context: context?.label, id });
            }}
          >
            <div className="search-dialog__header">
              <Icon name="search" className="size-4 shrink-0 text-muted" aria-hidden="true" />
              <Command.Input
                ref={input}
                autoFocus
                aria-label={inputLabel}
                placeholder={placeholder}
                value={query}
                onValueChange={onQueryChange}
                onKeyDownCapture={onInputKeyDown}
              />
              {query.length > 0 ? (
                <CloseButton
                  aria-label="Clear search"
                  onPress={() => {
                    onQueryChange("");
                    input.current?.focus();
                  }}
                />
              ) : null}
              <Modal.CloseTrigger className="search-dialog__close" aria-label="Close search" />
            </div>
            <Command.List ref={list} label={title} className="search-dialog__results">
              {visibleSections.map((section) => (
                <Command.Group key={section.id} heading={section.label} value={section.id}>
                  {section.items.map((item) => (
                    <Command.Item
                      key={item.id}
                      value={item.id}
                      onSelect={() => {
                        onAction(item.id);
                      }}
                    >
                      {item.icon === undefined ? null : (
                        <span aria-hidden="true" className="shrink-0 text-muted">
                          {item.icon}
                        </span>
                      )}
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm">{item.label}</span>
                        {item.description === undefined ? null : (
                          <span className="truncate text-xs text-muted">{item.description}</span>
                        )}
                      </div>
                      {item.metadata === undefined ? null : (
                        <span className="shrink-0 text-xs text-muted">{item.metadata}</span>
                      )}
                    </Command.Item>
                  ))}
                </Command.Group>
              ))}
              {visibleSections.length === 0 ? (
                <p className="search-dialog__empty">{emptyMessage}</p>
              ) : null}
            </Command.List>
          </Command>
          {visibleSections.length === 0 && emptyAction !== undefined ? (
            <div className="search-dialog__empty-action">
              <Button variant="secondary" onPress={emptyAction.onPress}>
                {emptyAction.label}
              </Button>
            </div>
          ) : null}
          {status === undefined ? null : <div className="search-dialog__status">{status}</div>}
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}
