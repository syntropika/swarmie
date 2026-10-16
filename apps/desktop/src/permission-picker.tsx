import { RadioGroup, Radio, Select, ListBox } from "@swarmie/ui-web";
import { Hand, ShieldCheck, ShieldAlert } from "lucide-react";
import { isValidElement, useId, useState } from "react";

import { startAction, errorReporter } from "~/src/action-boundary";
import type { Thread } from "~/src/protocol";
import { sessionPermissions } from "~/src/session-permissions";
import { hasText } from "~/src/value-presence";

export function PermissionPicker({
  thread,
  disabled,
  onChange,
}: {
  thread: Thread;
  disabled: boolean;
  onChange: (patch: Record<string, unknown>) => Promise<boolean>;
}) {
  const fieldId = useId();
  const [error, setError] = useState("");
  const { selected, options, status, observedLabel, policy } = sessionPermissions(thread);
  const nativeControls =
    thread.configurations?.filter(
      (field) => policy?.controlledSettings.includes(field.id) === true,
    ) ?? [];
  const icons = { ask: Hand, review: ShieldCheck, full: ShieldAlert };
  return (
    <>
      {hasText(error) && (
        <p role="alert" className="text-xs text-danger">
          {error}
        </p>
      )}
      <RadioGroup
        aria-label="Approval mode"
        value={selected ?? ""}
        orientation="vertical"
        onChange={(id) => {
          const option = options.find((entry) => entry.id === id);
          if (option?.available === true && !option.locked) {
            startAction(onChange(option.patch), errorReporter(setError));
          }
        }}
      >
        {options.map((option) => {
          const Icon = icons[option.id];
          return (
            <Radio
              key={option.id}
              value={option.id}
              isDisabled={disabled || !option.available || option.locked}
              aria-label={option.label}
            >
              <Radio.Content>
                <Radio.Control>
                  <Radio.Indicator />
                </Radio.Control>
                <Icon size={16} className="mt-0.5 shrink-0" />
                <span className="min-w-0 flex-1">
                  <span className="block">{option.label}</span>
                  <span
                    className={`block text-xs ${option.id === "full" ? "text-permission-full-access" : "text-muted"}`}
                  >
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
                </span>
              </Radio.Content>
            </Radio>
          );
        })}
      </RadioGroup>
      {hasText(status) && <output className="px-2.5 text-xs text-muted">{status}</output>}
      {policy?.status === "staged" && hasText(observedLabel) && (
        <p className="px-2.5 text-xs text-muted">Current native permissions: {observedLabel}</p>
      )}
      {policy?.observed?.limitations?.map((limitation) => (
        <p key={limitation} className="px-2.5 text-xs text-muted">
          {limitation}
        </p>
      ))}
      {nativeControls.length > 0 && (
        <details className="px-2.5 text-xs text-muted">
          <summary className="cursor-pointer py-2">Native permission settings</summary>
          <p className="mb-2">
            These settings can have different coverage from the common modes. An explicit change
            selects a custom native policy.
          </p>
          {nativeControls.map((field) => {
            const value =
              (hasText(thread.sessionId) ? undefined : thread.config?.[field.id]) ?? field.value;
            const locked = nativeControlLocked(disabled, thread, field.application);
            const selectedChoice =
              typeof value === "string" && field.choices?.some((choice) => choice.id === value);
            return (
              <div key={field.id} className="mb-2 block">
                <label htmlFor={`${fieldId}-${field.id}`}>{field.label}</label>
                {field.type === "select" && (field.choices?.length ?? 0) > 0 ? (
                  <Select
                    aria-label={`Native ${field.label}`}
                    value={
                      selectedChoice === true && typeof value === "string" ? value : "__custom"
                    }
                    onChange={(key) => {
                      if (typeof key === "string") {
                        ((next) => {
                          startAction(onChange({ [field.id]: next }), errorReporter(setError));
                        })(key);
                      }
                    }}
                    isDisabled={locked}
                    placeholder="Select…"
                  >
                    <Select.Trigger id={`${fieldId}-${field.id}`}>
                      <Select.Value />
                      <Select.Indicator />
                    </Select.Trigger>
                    <Select.Popover>
                      <ListBox aria-label={`Native ${field.label}`}>
                        {[
                          ...(selectedChoice === true
                            ? []
                            : [
                                {
                                  id: "__custom",
                                  label:
                                    value === undefined || value === null
                                      ? "Unknown"
                                      : "Current custom configuration",
                                  disabled: true,
                                },
                              ]),
                          ...(field.choices ?? []),
                        ].map((option) => (
                          <ListBox.Item
                            key={option.id}
                            id={option.id}
                            textValue={option.label}
                            isDisabled={"disabled" in option && option.disabled}
                          >
                            {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                            {option.label}
                            <ListBox.ItemIndicator />
                          </ListBox.Item>
                        ))}
                      </ListBox>
                    </Select.Popover>
                  </Select>
                ) : (
                  <pre className="overflow-auto whitespace-pre-wrap">
                    {JSON.stringify(value, null, 2)}
                  </pre>
                )}
                {value !== null && typeof value === "object" && (
                  <pre className="mt-1 overflow-auto whitespace-pre-wrap">
                    {JSON.stringify(value, null, 2)}
                  </pre>
                )}
              </div>
            );
          })}
        </details>
      )}
    </>
  );
}

function nativeControlLocked(disabled: boolean, thread: Thread, application: string) {
  return (
    disabled ||
    (hasText(thread.sessionId) &&
      (application === "creation_only" ||
        (thread.state !== "idle" && application !== "current_turn")))
  );
}
