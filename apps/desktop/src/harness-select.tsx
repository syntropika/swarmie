import { Tooltip, Button, Dropdown, Select, ListBox } from "@swarmie/ui-web";
import { Asterisk, ChevronDown, Sparkles, Terminal } from "lucide-react";
import { isValidElement } from "react";

import { harnessGuide } from "~/src/harness-setup";
import { HarnessStatus, HarnessStatusDot } from "~/src/harness-status";
import type { Detection } from "~/src/protocol";
import { hasText } from "~/src/value-presence";

export function HarnessIcon({ harness }: { harness: string }) {
  const Icon = (() => {
    if (harness === "claude") {
      return Asterisk;
    }
    if (harness === "gemini") {
      return Sparkles;
    }
    return Terminal;
  })();
  return <Icon size={14} aria-hidden="true" />;
}

const noDisabledHarnesses: string[] = [];

export function HarnessSelect({
  label,
  id,
  value,
  onChange,
  harnesses,
  disabledHarnesses = noDisabledHarnesses,
  disabled,
  className,
  inherited,
  supported,
  appearance = "select",
  status,
}: {
  label: string;
  id?: string;
  value: string;
  onChange: (value: string) => void;
  harnesses: Detection[];
  disabledHarnesses?: string[];
  disabled?: boolean;
  className?: string;
  inherited?: string;
  supported?: string[];
  appearance?: "select" | "menu";
  status?: { state: string; connected: boolean };
}) {
  const available = harnesses.filter((item) => availableHarness(item, supported));
  const installed = available.filter((item) => !disabledHarnesses.includes(item.harness));
  const selected = selectedHarness(installed, available, value, disabled);
  const emptyLabel = available.length > 0 ? "No agents enabled" : "No agents installed";
  if (appearance === "menu") {
    return (
      <Dropdown>
        <Tooltip>
          <Button
            id={id}
            variant="tertiary"

            size="sm"
            className={className}
            aria-label={`${label}: ${selected ? harnessGuide(selected.harness).name : emptyLabel}`}
            disabled={!status && (disabled === true || !installed.length)}
          >
            {status ? (
              <HarnessStatusDot {...status} />
            ) : (
              selected && <HarnessIcon harness={selected.harness} />
            )}
            <span>
              {(() => {
                if (selected) {
                  return harnessGuide(selected.harness).name;
                }
                if (installed.length) {
                  return "Choose an agent";
                }
                return emptyLabel;
              })()}
            </span>
            <ChevronDown size={14} />
          </Button>
          {status && <HarnessStatus {...status} />}
        </Tooltip>
        <Dropdown.Popover className="min-w-44" placement="top end">
          <Dropdown.Menu
            aria-label={label}
            selectionMode="single"
            selectedKeys={selected ? new Set([selected.harness]) : new Set()}
            onAction={(key) => {
              if (typeof key === "string" && key !== value) {
                onChange(key);
              }
            }}
          >
            {!installed.length && (
              <p className="px-2.5 py-2 text-xs text-muted">
                Enable an agent tool in Settings to start a new chat.
              </p>
            )}
            {installed.map((item) => (
              <Dropdown.Item
                key={item.harness}
                id={item.harness}
                textValue={harnessGuide(item.harness).name}
                isDisabled={disabled}
              >
                <HarnessIcon harness={item.harness} />
                {harnessGuide(item.harness).name}
                <Dropdown.ItemIndicator />
              </Dropdown.Item>
            ))}
          </Dropdown.Menu>
        </Dropdown.Popover>
      </Dropdown>
    );
  }
  return (
    <Select
      aria-label={label}
      value={selected ? value : ""}
      onChange={(key) => {
        if (typeof key === "string") {
          onChange(key);
        }
      }}
      isDisabled={disabled === true || (!installed.length && !hasText(inherited))}
      placeholder={installed.length ? "Choose an agent" : emptyLabel}
      className={className}
    >
      <Select.Trigger id={id}>
        {selected && <HarnessIcon harness={selected.harness} />}
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover className="min-w-44">
        <ListBox aria-label={label}>
          {[
            ...(hasText(inherited) ? [{ id: "", label: inherited }] : []),
            ...installed.map((item) => ({
              id: item.harness,
              label: harnessGuide(item.harness).name,
              icon: <HarnessIcon harness={item.harness} />,
            })),
          ].map((option) => (
            <ListBox.Item
              key={option.id}
              id={option.id}
              textValue={option.label}
              isDisabled={"disabled" in option && option.disabled === true}
            >
              {"icon" in option && isValidElement(option.icon) ? option.icon : null}
              {option.label}
              <ListBox.ItemIndicator />
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  );
}

function availableHarness(item: Detection, supported?: string[]) {
  return item.available && (supported === undefined || supported.includes(item.harness));
}

function selectedHarness(
  installed: Detection[],
  available: Detection[],
  value: string,
  disabled?: boolean,
) {
  return (
    installed.find((item) => item.harness === value) ??
    (disabled === true ? available.find((item) => item.harness === value) : undefined)
  );
}
