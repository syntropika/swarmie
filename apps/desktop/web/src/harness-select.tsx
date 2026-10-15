import { Asterisk, ChevronDown, Sparkles, Terminal } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Dropdown } from "@/components/ui/dropdown";
import { Select } from "@/components/ui/select";

import { harnessGuide } from "./harness-setup";
import { HarnessStatus, HarnessStatusDot } from "./harness-status";
import type { Detection } from "./protocol";
import { hasText } from "./value-presence";

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
      <Dropdown triggerTooltip={status && <HarnessStatus {...status} />}>
        <Button
          id={id}
          variant="composer"
          shape="pill"
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
        <Dropdown.Popover className="min-w-44" placement="top end">
          <Dropdown.Menu
            aria-label={label}
            selectionMode="single"
            selectedKeys={selected ? new Set([selected.harness]) : new Set()}
            onAction={(key) => {
              if (key !== value) {
                onChange(key);
              }
            }}
          >
            {!installed.length && (
              <p className="text-muted px-2.5 py-2 text-xs">
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
      id={id}
      label={label}
      value={selected ? value : ""}
      onChange={onChange}
      className={className}
      popoverClassName="min-w-44"
      disabled={disabled === true || (!installed.length && !hasText(inherited))}
      placeholder={installed.length ? "Choose an agent" : emptyLabel}
      icon={selected && <HarnessIcon harness={selected.harness} />}
      options={[
        ...(hasText(inherited) ? [{ id: "", label: inherited }] : []),
        ...installed.map((item) => ({
          id: item.harness,
          label: harnessGuide(item.harness).name,
          icon: <HarnessIcon harness={item.harness} />,
        })),
      ]}
    />
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
