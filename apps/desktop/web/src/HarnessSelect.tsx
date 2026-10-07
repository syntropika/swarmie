import { Asterisk, ChevronDown, Sparkles, Terminal } from "lucide-react";
import { Dropdown } from "@/components/ui/dropdown";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { HarnessStatus, HarnessStatusDot } from "./HarnessStatus";
import type { Detection } from "./protocol";

export function HarnessIcon({ harness }: { harness: string }) {
  const Icon = harness === "claude" ? Asterisk : harness === "gemini" ? Sparkles : Terminal;
  return <Icon size={14} aria-hidden="true" />;
}

export function HarnessSelect({ label, value, onChange, harnesses, disabled, className, inherited, supported, appearance = "select", status }: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  harnesses: Detection[];
  disabled?: boolean;
  className?: string;
  inherited?: string;
  supported?: string[];
  appearance?: "select" | "menu";
  status?: { state: string; connected: boolean };
}) {
  const installed = harnesses.filter((item) => item.available && (!supported || supported.includes(item.harness)));
  const selected = installed.find((item) => item.harness === value);
  if (appearance === "menu") return <Dropdown triggerTooltip={status && <HarnessStatus {...status} />}>
    <Button variant="ghost" size="sm" className={`rounded-full hover:bg-[#4a4a4a] active:bg-[#4a4a4a] ${className ?? ""}`} aria-label={`${label}: ${selected?.harness ?? "No harness installed"}`}
      disabled={!status && (disabled || !installed.length)}>
      {status ? <HarnessStatusDot {...status} /> : selected && <HarnessIcon harness={selected.harness} />}
      <span>{selected?.harness ?? (installed.length ? "Select a harness" : "No harness installed")}</span><ChevronDown size={14} />
    </Button>
    <Dropdown.Popover className="harness-menu" placement="top end">
      <Dropdown.Menu aria-label={label} selectionMode="single" selectedKeys={selected ? new Set([selected.harness]) : new Set()}
        onAction={(key) => { if (String(key) !== value) onChange(String(key)); }}>
        {!installed.length && <p className="px-2.5 py-2 text-xs text-muted">No harness is available on this computer.</p>}
        {installed.map((item) => <Dropdown.Item key={item.harness} id={item.harness} textValue={item.harness} isDisabled={disabled}>
          <HarnessIcon harness={item.harness} />{item.harness}<Dropdown.ItemIndicator />
        </Dropdown.Item>)}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>;
  return <Select label={label} value={selected ? value : ""} onChange={onChange}
    className={className} popoverClassName="harness-menu" disabled={disabled || (!installed.length && !inherited)}
    placeholder={installed.length ? "Select a harness" : "No harness installed"}
    icon={selected && <HarnessIcon harness={selected.harness} />}
    options={[
      ...(inherited ? [{ id: "", label: inherited }] : []),
      ...installed.map((item) => ({ id: item.harness, label: item.harness, icon: <HarnessIcon harness={item.harness} /> })),
    ]} />;
}
