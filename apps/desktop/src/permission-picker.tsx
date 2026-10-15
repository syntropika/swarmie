import * as Radio from "@radix-ui/react-radio-group";
import { Check, Hand, ShieldCheck, ShieldAlert } from "lucide-react";
import { useId, useState } from "react";

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
      <Radio.Root
        aria-label="Approval mode"
        value={selected ?? ""}
        orientation="vertical"
        className="grid gap-1"
        onValueChange={(id) => {
          const option = options.find((entry) => entry.id === id);
          if (option?.available === true && !option.locked) {
            startAction(onChange(option.patch), errorReporter(setError));
          }
        }}
      >
        {options.map((option) => {
          const Icon = icons[option.id];
          return (
            <Radio.Item
              key={option.id}
              value={option.id}
              disabled={disabled || !option.available || option.locked}
              aria-label={option.label}
              className={`hover:bg-control-hover focus-visible:bg-control-hover focus-visible:ring-focus flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-40 ${option.id === "full" ? "text-permission-full-access" : "text-foreground"}`}
            >
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
              <Radio.Indicator className="mt-0.5">
                <Check size={16} />
              </Radio.Indicator>
            </Radio.Item>
          );
        })}
      </Radio.Root>
      {hasText(status) && <output className="text-muted px-2.5 text-xs">{status}</output>}
      {policy?.status === "staged" && hasText(observedLabel) && (
        <p className="text-muted px-2.5 text-xs">Current native permissions: {observedLabel}</p>
      )}
      {policy?.observed?.limitations?.map((limitation) => (
        <p key={limitation} className="text-muted px-2.5 text-xs">
          {limitation}
        </p>
      ))}
      {nativeControls.length > 0 && (
        <details className="text-muted px-2.5 text-xs">
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
                  <select
                    id={`${fieldId}-${field.id}`}
                    aria-label={`Native ${field.label}`}
                    className="bg-control-secondary text-foreground mt-1 block w-full rounded p-1"
                    value={
                      selectedChoice === true && typeof value === "string" ? value : "__custom"
                    }
                    disabled={locked}
                    onChange={(event) => {
                      startAction(
                        onChange({ [field.id]: event.target.value }),
                        errorReporter(setError),
                      );
                    }}
                  >
                    {!(selectedChoice === true) && (
                      <option value="__custom" disabled>
                        {value === undefined || value === null
                          ? "Unknown"
                          : "Current custom configuration"}
                      </option>
                    )}
                    {field.choices?.map((choice) => (
                      <option key={choice.id} value={choice.id}>
                        {choice.label}
                      </option>
                    ))}
                  </select>
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
