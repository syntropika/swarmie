import { useSource } from "@swarmie/client-ui";
import { Button, Label, ListBox, Select } from "@swarmie/ui-web";
import { ArrowLeft, Puzzle } from "lucide-react";

import { useClient } from "../client/context";
import { ModelProtectionSettings, VaultSettings } from "../vault/settings";
import { ConnectionSettings } from "./connections";
import { GeneralSettings } from "./general";
import { ProviderSettings } from "./providers";

function PluginConfiguration({ id }: { id: string }) {
  const { settings } = useClient();
  const preferences = useSource(settings);
  if (id === "vault") {
    return <VaultSettings />;
  }
  if (id === "model-protection") {
    return <ModelProtectionSettings />;
  }
  if (id === "connections") {
    return <ConnectionSettings />;
  }
  if (id === "settings") {
    return <GeneralSettings />;
  }
  if (id === "providers") {
    return (
      <div className="mt-8">
        <ProviderSettings />
      </div>
    );
  }
  if (id !== "read-aloud") {
    return null;
  }
  return (
    <div className="mt-8">
      <Select
        value={preferences.language}
        onChange={(value) => {
          if (typeof value === "string") {
            settings.update((current) => ({ ...current, language: value }));
          }
        }}
      >
        <Label>Language</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            {(
              [
                ["en-US", "English"],
                ["es-ES", "Spanish"],
                ["fr-FR", "French"],
                ["de-DE", "German"],
                ["pt-PT", "Portuguese"],
              ] as const
            ).map(([code, label]) => (
              <ListBox.Item key={code} id={code} textValue={label}>
                {label}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
      <p className="mt-3 text-xs text-muted">
        Speech uses your browser. Code blocks are skipped when reading messages.
      </p>
    </div>
  );
}
export function PluginDetail({
  id,
  back,
  openPlugin,
}: {
  id: string;
  back: () => void;
  openPlugin: (id: string | null) => void;
}) {
  const { plugins } = useClient();
  const list = useSource(plugins);
  const plugin = list.find((entry) => entry.id === id);
  if (plugin === undefined) {
    return null;
  }

  return (
    <>
      <Button
        size="sm"
        variant="ghost"
        onPress={() => {
          back();
        }}
      >
        <ArrowLeft />
        Back to plugins
      </Button>
      <h1 className="mt-7 text-2xl font-medium">{plugin.name}</h1>
      <p className="mt-2 text-sm leading-6 text-muted">{plugin.description}</p>
      <PluginConfiguration id={plugin.id} />
      <h2 className="mt-10 mb-3 text-sm">Permissions</h2>
      <p className="text-xs leading-5 text-muted">
        {plugin.core
          ? "Bundled core plugin. Permissions are available for inspection and do not require acceptance."
          : "Review access before enabling this plugin. Additional permissions require a new acceptance."}
      </p>
      <ul className="mt-3 list-inside list-disc space-y-2 text-xs">
        {plugin.permissions.map((permission) => (
          <li key={permission}>{permission}</li>
        ))}
      </ul>
      {plugin.uses.length > 0 ? (
        <>
          <h2 className="mt-10 mb-3 text-sm">Uses</h2>
          <div className="grid gap-1 rounded-group bg-group p-1">
            {plugin.uses.map((name) => (
              <Button
                key={name}
                variant="ghost"
                onPress={() => {
                  openPlugin(list.find((entry) => entry.name === name)?.id ?? null);
                }}
                className="w-full justify-start text-left"
              >
                <Puzzle />
                <span className="min-w-0 truncate">{name}</span>
              </Button>
            ))}
          </div>
        </>
      ) : null}
      <h2 className="mt-10 mb-3 text-sm">About</h2>
      <div className="divide-y divide-border-subtle rounded-group bg-group px-4">
        {[
          ["Category", plugin.category],
          ["Developer", "Swarmie"],
          ["Version", plugin.version],
        ].map(([label, value]) => (
          <div key={label} className="grid grid-cols-2 gap-3 py-4 text-xs wrap-anywhere">
            <span className="text-muted">{label}</span>
            <span>{value}</span>
          </div>
        ))}
      </div>
      {plugin.errors.map((failure) => (
        <p key={failure} role="alert" className="mt-3 text-xs text-danger">
          {failure}
        </p>
      ))}
    </>
  );
}
