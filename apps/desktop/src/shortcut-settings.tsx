import { Button, Kbd, Modal, Table, Tooltip, TooltipContent } from "@swarmie/ui-web";
import { Pencil, RotateCcw } from "lucide-react";
import { useEffect, useState } from "react";

import { defaultShortcuts, shortcutBinding } from "~/src/shortcuts";
import type { Shortcuts } from "~/src/shortcuts";

const actions = [
  ["palette", "Command palette"],
  ["newChat", "New conversation"],
  ["files", "Find file"],
  ["tools", "Workspace tools"],
] as const;

export function ShortcutSettings({
  shortcuts,
  onChange,
}: {
  shortcuts: Shortcuts;
  onChange: (value: Shortcuts) => void;
}) {
  const [editing, setEditing] = useState<keyof Shortcuts | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const save = (next: Shortcuts) => {
    try {
      persistShortcuts(next);
      onChange(next);
      setErrorMessage(null);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Could not save shortcuts.");
    }
  };

  useEffect(() => {
    if (editing === null) {
      return undefined;
    }
    const record = (event: KeyboardEvent) => {
      if (event.key === "Tab" || event.key === "Escape") {
        return;
      }
      if (
        (event.key === "Enter" || event.key === " ") &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey &&
        event.target instanceof HTMLButtonElement
      ) {
        return;
      }
      event.stopPropagation();
      event.preventDefault();
      if (event.repeat) {
        return;
      }
      try {
        const binding = shortcutBinding(event);
        if (binding !== undefined) {
          const next = { ...shortcuts, [editing]: binding };
          persistShortcuts(next);
          onChange(next);
          setErrorMessage(null);
          setEditing(null);
        }
      } catch (error) {
        setErrorMessage(error instanceof Error ? error.message : "Could not save shortcuts.");
      }
    };
    window.addEventListener("keydown", record, true);
    return () => {
      window.removeEventListener("keydown", record, true);
    };
  }, [editing, shortcuts, onChange]);

  return (
    <div className="settings-section grid gap-4">
      <p className="muted">Choose the pencil to record a shortcut. Escape cancels editing.</p>
      <Table>
        <Table.ScrollContainer>
          <Table.Content aria-label="Keyboard shortcuts">
            <Table.Header>
              <Table.Column isRowHeader>Action</Table.Column>
              <Table.Column>Shortcut</Table.Column>
              <Table.Column>
                <span className="sr-only">Actions</span>
              </Table.Column>
            </Table.Header>
            <Table.Body>
              {actions.map(([key, label]) => (
                <Table.Row key={key} id={key}>
                  <Table.Cell>{label}</Table.Cell>
                  <Table.Cell>
                    <ShortcutKeys binding={shortcuts[key]} />
                  </Table.Cell>
                  <Table.Cell>
                    <div className="flex justify-end gap-1">
                      <Tooltip delay={500}>
                        <Button
                          variant="tertiary"
                          size="md"
                          isIconOnly
                          aria-label={`Edit shortcut for ${label}`}
                          onClick={() => {
                            setErrorMessage(null);
                            setEditing(key);
                          }}
                        >
                          <Pencil size={16} />
                        </Button>
                        <TooltipContent>Edit shortcut</TooltipContent>
                      </Tooltip>
                      <Tooltip delay={500}>
                        <Button
                          variant="tertiary"
                          size="md"
                          isIconOnly
                          aria-label={`Reset shortcut for ${label}`}
                          isDisabled={shortcuts[key] === defaultShortcuts[key]}
                          onClick={() => {
                            save({ ...shortcuts, [key]: defaultShortcuts[key] });
                          }}
                        >
                          <RotateCcw size={16} />
                        </Button>
                        <TooltipContent>Restore default shortcut</TooltipContent>
                      </Tooltip>
                    </div>
                  </Table.Cell>
                </Table.Row>
              ))}
            </Table.Body>
          </Table.Content>
        </Table.ScrollContainer>
      </Table>
      {errorMessage !== null && editing === null && (
        <p role="alert" className="text-danger">
          {errorMessage}
        </p>
      )}
      <Modal.Backdrop
        isDismissable
        isOpen={editing !== null}
        onOpenChange={(open) => {
          if (!open) {
            setEditing(null);
            setErrorMessage(null);
          }
        }}
      >
        <Modal.Container size="sm">
          <Modal.Dialog
            aria-label={`Record shortcut for ${actions.find(([key]) => key === editing)?.[1] ?? "action"}`}
          >
            <Modal.CloseTrigger />
            <Modal.Header>
              <Modal.Heading>Edit shortcut</Modal.Heading>
            </Modal.Header>
            <Modal.Body>
              <div className="grid gap-4">
                <p>{actions.find(([key]) => key === editing)?.[1]}</p>
                {editing !== null && <ShortcutKeys binding={shortcuts[editing]} />}
                <p>Press a shortcut…</p>
                <p className="muted">Press a key combination. Escape cancels editing.</p>
                {errorMessage !== null && (
                  <p role="alert" className="text-danger">
                    {errorMessage}
                  </p>
                )}
              </div>
            </Modal.Body>
            <Modal.Footer>
              <Button variant="tertiary" size="md" slot="close">
                Cancel
              </Button>
            </Modal.Footer>
          </Modal.Dialog>
        </Modal.Container>
      </Modal.Backdrop>
      <div>
        <Button
          variant="tertiary"
          size="md"
          onClick={() => {
            try {
              localStorage.removeItem("swarmie.shortcuts");
              onChange({ ...defaultShortcuts });
              setErrorMessage(null);
            } catch (error) {
              setErrorMessage(
                error instanceof Error ? error.message : "Could not reset shortcuts.",
              );
            }
          }}
        >
          Reset all shortcuts
        </Button>
      </div>
    </div>
  );
}

function ShortcutKeys({ binding }: { binding: string }) {
  const mac = /Mac|iPhone|iPad/u.test(navigator.platform);
  return (
    <Kbd aria-label={binding.replace("Mod", mac ? "Command" : "Control")}>
      {binding.split("+").map((part) => {
        if (part === "Mod" || part === "Shift" || part === "Alt") {
          const modifiers = { Mod: mac ? "command" : "ctrl", Shift: "shift", Alt: "alt" } as const;
          const keyValue = modifiers[part];
          return <Kbd.Abbr key={part} keyValue={keyValue} />;
        }
        return <Kbd.Content key={part}>{part === " " ? "Space" : part}</Kbd.Content>;
      })}
    </Kbd>
  );
}

function persistShortcuts(next: Shortcuts) {
  const bindings = Object.values(next).map((binding) => binding.toLowerCase());
  if (new Set(bindings).size !== bindings.length) {
    throw new Error("Choose a shortcut not assigned to another action.");
  }
  localStorage.setItem("swarmie.shortcuts", JSON.stringify(next));
}
