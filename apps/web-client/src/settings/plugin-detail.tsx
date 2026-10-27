import { useSource } from "@swarmie/client-ui";
import { Button, ChromeButton, Label, ListBox, Select } from "@swarmie/ui-web";
import { ArrowLeft, Puzzle } from "lucide-react";

import { useClient } from "../client/context";
import { iconStyle } from "../controls";
import { ConnectionSettings } from "./connections";
import { GeneralSettings } from "./general";
import { ProviderSettings } from "./providers";

function PluginConfiguration({ id }: { id: string }) {
  const { settings, onboarding } = useClient();
  const preferences = useSource(settings);
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
  if (id === "onboarding") {
    return (
      <div className="mt-6">
        <Button
          variant="secondary"
          onPress={() => {
            onboarding.update((current) => ({ ...current, open: true }));
          }}
        >
          Open getting started
        </Button>
      </div>
    );
  }
  if (id !== "read-aloud") {
    return null;
  }
  return (
    <div className="mt-8 rounded-xl border border-border p-4">
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
      <ChromeButton
        size="action"
        onPress={() => {
          back();
        }}
      >
        <ArrowLeft className={iconStyle} />
        Back to plugins
      </ChromeButton>
      <h1 className="mt-7 text-2xl font-medium">{plugin.name}</h1>
      <p className="mt-2 text-sm leading-6 text-muted">{plugin.description}</p>
      <PluginConfiguration id={plugin.id} />
      {plugin.uses.length > 0 ? (
        <>
          <h2 className="mt-10 mb-3 text-sm">Uses</h2>
          <div className="divide-y divide-border rounded-xl border border-border bg-chrome-surface">
            {plugin.uses.map((name) => (
              <button
                key={name}
                type="button"
                onClick={() => {
                  openPlugin(list.find((entry) => entry.name === name)?.id ?? null);
                }}
                className="flex w-full items-center gap-3 px-4 py-4 text-left text-sm hover:bg-row-hover"
              >
                <Puzzle className="size-4" />
                {name}
              </button>
            ))}
          </div>
        </>
      ) : null}
      <h2 className="mt-10 mb-3 text-sm">About</h2>
      <div className="divide-y divide-border rounded-xl border border-border bg-chrome-surface px-4">
        {[
          ["Category", plugin.category],
          ["Developer", "Swarmie"],
          ["Version", plugin.version],
        ].map(([label, value]) => (
          <div key={label} className="grid grid-cols-2 py-4 text-xs">
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
