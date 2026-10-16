import { Description, Label, ListBox } from "@swarmie/ui-web";
import { Hand, Shield, ShieldCheck, ShieldAlert } from "lucide-react";
import { useState } from "react";

import { startAction, errorReporter } from "~/src/action-boundary";
import type { Thread } from "~/src/protocol";
import { sessionPermissions } from "~/src/session-permissions";
import type { PermissionMode } from "~/src/session-permissions";
import { hasText } from "~/src/value-presence";

const permissionIcons = { ask: Hand, review: ShieldCheck, full: ShieldAlert };

export function PermissionIcon({ mode, size }: { mode: PermissionMode | undefined; size: number }) {
  const Icon = mode === undefined ? Shield : permissionIcons[mode];
  return <Icon size={size} className="shrink-0" aria-hidden="true" />;
}

export function PermissionPicker({
  thread,
  disabled,
  onChange,
}: {
  thread: Thread;
  disabled: boolean;
  onChange: (patch: Record<string, unknown>) => Promise<boolean>;
}) {
  const [error, setError] = useState("");
  const { selected, options } = sessionPermissions(thread);
  return (
    <>
      {hasText(error) && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
      <ListBox
        aria-label="Approval mode"
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={selected ? [selected] : []}
        disabledKeys={options
          .filter((option) => disabled || !option.available || option.locked)
          .map((option) => option.id)}
        onSelectionChange={(keys) => {
          if (keys === "all") {
            return;
          }
          const id = keys.values().next().value;
          const option = options.find((entry) => entry.id === id);
          if (option?.available === true && !option.locked) {
            startAction(onChange(option.patch), errorReporter(setError));
          }
        }}
      >
        {options.map((option) => (
          <ListBox.Item
            key={option.id}
            id={option.id}
            textValue={option.label}
            aria-label={option.label}
          >
            <PermissionIcon mode={option.id} size={16} />
            <div className="flex min-w-0 flex-1 flex-col">
              <Label>{option.label}</Label>
              <Description>
                <span className={option.id === "full" ? "text-permission-full-access" : undefined}>
                  {(() => {
                    if (option.available) {
                      if (option.locked) {
                        return "Wait until this session is idle";
                      }
                      return option.description;
                    }
                    return option.reason ?? "Not supported by this agent tool";
                  })()}
                </span>
              </Description>
            </div>
            <ListBox.ItemIndicator />
          </ListBox.Item>
        ))}
      </ListBox>
    </>
  );
}
