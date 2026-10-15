import type { McpEntry } from "@swarmie/contracts";
import { Settings2, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";

import { hasText } from "./value-presence";

const statusLabels = new Map<string, string>([
  ["notLoggedIn", "Sign-in required"],
  ["authenticationRequired", "Sign-in required"],
  ["needs-auth", "Sign-in required"],
  ["needs_auth", "Sign-in required"],
  ["unknown", "Not observed"],
  ["tools-available", "Tools available"],
  ["bearerToken", "Bearer token"],
  ["oAuth", "OAuth"],
  ["connected", "Connected"],
  ["ready", "Ready"],
  ["failed", "Connection failed"],
  ["disabled", "Disabled"],
  ["unsupported", "Not supported"],
]);
const status = (value: string) => statusLabels.get(value) ?? value;
function desiredStatus(entry: McpEntry) {
  if (entry.scope === "session") {
    return "Native session";
  }
  return entry.enabled ? "Enabled" : "Disabled";
}

export function McpServerRow({
  entry,
  busy,
  confirming,
  onToggle,
  onEdit,
  onAuthenticate,
  onRemove,
  onConfirm,
  onKeep,
}: {
  entry: McpEntry;
  busy: string;
  confirming: boolean;
  onToggle: (enabled: boolean) => void;
  onEdit: () => void;
  onAuthenticate: () => void;
  onRemove: () => void;
  onConfirm: () => void;
  onKeep: () => void;
}) {
  return (
    <div className="grid gap-3 py-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-sm font-medium break-words">{entry.name}</h2>
          <p className="text-muted text-xs">
            {entry.origin === "swarmie"
              ? `Swarmie · ${entry.harness ?? "all compatible harnesses"}`
              : entry.harness}{" "}
            · {entry.scope} ·{" "}
            {entry.definition.transport === "unknown"
              ? "Transport not reported"
              : entry.definition.transport.toUpperCase()}
          </p>
        </div>
        {entry.scope !== "session" && (
          <Switch
            label={`Enable ${entry.name}`}
            hideLabel
            checked={entry.enabled}
            disabled={!!busy || !entry.actions.includes(entry.enabled ? "disable" : "enable")}
            onChange={onToggle}
          />
        )}
      </div>
      <p className="text-sm">
        {desiredStatus(entry)} · Connection: {status(entry.runtimeStatus)} · Authentication:{" "}
        {status(entry.authStatus)}
      </p>
      {hasText(entry.source) && <p className="text-muted text-xs break-all">{entry.source}</p>}
      {hasText(entry.projectPath) && (
        <p className="text-muted text-xs break-all">Project: {entry.projectPath}</p>
      )}
      {hasText(entry.readOnlyReason) && (
        <p className="text-muted text-sm">{entry.readOnlyReason}</p>
      )}
      {entry.requiresRestart && (
        <p className="text-sm">Reopen the native session to apply its saved configuration.</p>
      )}
      {!!entry.tools.length && (
        <details>
          <summary className="cursor-pointer text-sm">{entry.tools.length} available tools</summary>
          <ul className="mt-2 grid gap-1 text-sm">
            {entry.tools.map((tool) => (
              <li key={tool} className="break-all">
                {tool}
              </li>
            ))}
          </ul>
        </details>
      )}
      <div className="flex flex-wrap gap-2">
        {entry.actions.includes("update") && (
          <Button variant="outline" disabled={!!busy} onClick={onEdit}>
            <Settings2 size={14} />
            Edit
          </Button>
        )}
        {entry.actions.includes("authenticate") && (
          <Button variant="outline" disabled={!!busy} onClick={onAuthenticate}>
            Sign in
          </Button>
        )}
        {entry.actions.includes("uninstall") && (
          <Button variant="outline" disabled={!!busy} onClick={onRemove}>
            <Trash2 size={14} />
            Uninstall
          </Button>
        )}
      </div>
      {confirming && (
        <fieldset className="grid gap-2 text-sm" aria-label={`Remove ${entry.name}`}>
          <p>
            Remove this MCP configuration from{" "}
            {entry.origin === "swarmie" ? "Swarmie" : entry.harness}? Its executable and user files
            will be kept.
          </p>
          <div className="flex gap-2">
            <Button disabled={!!busy} onClick={onConfirm}>
              Confirm uninstall
            </Button>
            <Button variant="outline" disabled={!!busy} onClick={onKeep}>
              Keep server
            </Button>
          </div>
        </fieldset>
      )}
    </div>
  );
}
