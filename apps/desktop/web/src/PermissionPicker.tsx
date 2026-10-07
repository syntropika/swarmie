import * as Radio from "@radix-ui/react-radio-group";
import { Check, Hand, ShieldCheck, ShieldAlert } from "lucide-react";
import type { Thread } from "./protocol";
import { sessionPermissions } from "./sessionPermissions";

export function PermissionPicker({ thread, disabled, onChange }: { thread: Thread; disabled: boolean; onChange: (patch: Record<string, unknown>) => Promise<boolean> }) {
  const { selected, options } = sessionPermissions(thread);
  const icons = { ask: Hand, review: ShieldCheck, full: ShieldAlert };
  return <Radio.Root aria-label="Approval mode" value={selected ?? ""} orientation="vertical" className="grid gap-1"
    onValueChange={(id) => { const option = options.find((entry) => entry.id === id); if (option?.available && !option.locked) void onChange(option.patch); }}>
    {options.map((option) => { const Icon = icons[option.id]; return <Radio.Item key={option.id} value={option.id} disabled={disabled || !option.available || option.locked}
      aria-label={option.label} className={`flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm outline-none hover:bg-control-hover focus-visible:bg-control-hover focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-40 ${option.id === "full" ? "text-[#ff984e]" : "text-foreground"}`}>
      <Icon size={16} className="mt-0.5 shrink-0" /><span className="min-w-0 flex-1"><span className="block">{option.label}</span><span className={`block text-xs ${option.id === "full" ? "text-[#ff984e]" : "text-muted"}`}>{!option.available ? "Not supported by this harness" : option.locked ? "Set before starting a session" : option.description}</span></span>
      <Radio.Indicator className="mt-0.5"><Check size={16} /></Radio.Indicator>
    </Radio.Item>; })}
  </Radio.Root>;
}
