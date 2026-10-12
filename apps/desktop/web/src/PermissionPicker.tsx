import * as Radio from "@radix-ui/react-radio-group";
import { Check, Hand, ShieldCheck, ShieldAlert } from "lucide-react";
import type { Thread } from "./protocol";
import { sessionPermissions } from "./sessionPermissions";

export function PermissionPicker({ thread, disabled, onChange }: { thread: Thread; disabled: boolean; onChange: (patch: Record<string, unknown>) => Promise<boolean> }) {
  const { selected, options, status, observedLabel, policy } = sessionPermissions(thread);
  const nativeControls = thread.configurations?.filter((field) => policy?.controlledSettings.includes(field.id)) ?? [];
  const icons = { ask: Hand, review: ShieldCheck, full: ShieldAlert };
  return <><Radio.Root aria-label="Approval mode" value={selected ?? ""} orientation="vertical" className="grid gap-1"
    onValueChange={(id) => { const option = options.find((entry) => entry.id === id); if (option?.available && !option.locked) void onChange(option.patch); }}>
    {options.map((option) => { const Icon = icons[option.id]; return <Radio.Item key={option.id} value={option.id} disabled={disabled || !option.available || option.locked}
      aria-label={option.label} className={`flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm outline-none hover:bg-control-hover focus-visible:bg-control-hover focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40 ${option.id === "full" ? "text-[#ff984e]" : "text-foreground"}`}>
      <Icon size={16} className="mt-0.5 shrink-0" /><span className="min-w-0 flex-1"><span className="block">{option.label}</span><span className={`block text-xs ${option.id === "full" ? "text-[#ff984e]" : "text-muted"}`}>{!option.available ? option.reason ?? "Not supported by this harness" : option.locked ? "Wait until this session is idle" : option.description}</span></span>
      <Radio.Indicator className="mt-0.5"><Check size={16} /></Radio.Indicator>
    </Radio.Item>; })}
  </Radio.Root>{status && <p className="px-2.5 text-xs text-muted" role="status">{status}</p>}
    {policy?.status === "staged" && observedLabel && <p className="px-2.5 text-xs text-muted">Current native permissions: {observedLabel}</p>}
    {policy?.observed?.limitations?.map((limitation) => <p key={limitation} className="px-2.5 text-xs text-muted">{limitation}</p>)}
    {nativeControls.length > 0 && <details className="px-2.5 text-xs text-muted">
      <summary className="cursor-pointer py-2">Native permission settings</summary>
      <p className="mb-2">These settings can have different coverage from the common modes. An explicit change selects a custom native policy.</p>
      {nativeControls.map((field) => {
        const value = (!thread.sessionId ? thread.config?.[field.id] : undefined) ?? field.value;
        const locked = disabled || (!!thread.sessionId && (field.application === "creation_only" || (thread.state !== "idle" && field.application !== "current_turn")));
        const selectedChoice = typeof value === "string" && field.choices?.some((choice) => choice.id === value);
        return <label key={field.id} className="mb-2 block">{field.label}
          {field.type === "select" && field.choices?.length ? <select aria-label={`Native ${field.label}`} className="mt-1 block w-full rounded bg-control p-1 text-foreground"
            value={selectedChoice ? String(value) : "__custom"} disabled={locked}
            onChange={(event) => void onChange({ [field.id]: event.target.value })}>
            {!selectedChoice && <option value="__custom" disabled>{value == null ? "Unknown" : "Current custom configuration"}</option>}
            {field.choices.map((choice) => <option key={choice.id} value={choice.id}>{choice.label}</option>)}
          </select> : <pre className="overflow-auto whitespace-pre-wrap">{JSON.stringify(value, null, 2)}</pre>}
          {value !== null && typeof value === "object" && <pre className="mt-1 overflow-auto whitespace-pre-wrap">{JSON.stringify(value, null, 2)}</pre>}
        </label>;
      })}
    </details>}
  </>;
}
