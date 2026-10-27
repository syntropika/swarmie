import { useSource } from "@swarmie/client-ui";
import { Button, ChromeButton, Switch } from "@swarmie/ui-web";
import { ArrowLeft, Bell, Code2, Folder, Globe, Mic, Puzzle, Settings } from "lucide-react";
import { useState } from "react";

import { useClient } from "../client/context";
import { IconButton, iconStyle } from "../controls";
import { ConnectionSettings } from "./connections";
import { GeneralSettings } from "./general";
import { ProviderSettings } from "./providers";

const icons = {
  projects: Folder,
  notifications: Bell,
  "read-aloud": Mic,
  views: Globe,
  "app-state": Code2,
  providers: Puzzle,
};
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
      <label className="grid gap-3 text-sm">
        Language
        <select
          value={preferences.language}
          onChange={(event) => {
            settings.update((current) => ({ ...current, language: event.target.value }));
          }}
          className="rounded-lg border border-border bg-default px-3 py-2"
        >
          {[
            ["en-US", "English"],
            ["es-ES", "Spanish"],
            ["fr-FR", "French"],
            ["de-DE", "German"],
            ["pt-PT", "Portuguese"],
          ].map(([code, label]) => (
            <option key={code} value={code}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <p className="mt-3 text-xs text-muted">
        Speech uses your browser. Code blocks are skipped when reading messages.
      </p>
    </div>
  );
}
export function PluginSettings() {
  const { plugins, togglePlugin } = useClient();
  const list = useSource(plugins);
  const [detail, setDetail] = useState<string | null>(null);
  const plugin = list.find((entry) => entry.id === detail);
  if (plugin !== undefined) {
    return (
      <>
        <ChromeButton
          size="action"
          onPress={() => {
            setDetail(null);
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
                    setDetail(list.find((entry) => entry.name === name)?.id ?? null);
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
  return (
    <>
      <h1 className="text-2xl font-medium">Plugins</h1>
      <p className="mt-2 text-xs text-muted">Manage the plugins that make Swarmie yours.</p>
      <div className="mt-8 divide-y divide-border rounded-xl border border-border bg-chrome-surface px-4">
        {list.map((entry) => {
          const Mark = Object.entries(icons).find(([id]) => id === entry.id)?.[1] ?? Puzzle;
          return (
            <div key={entry.id} className="flex items-center gap-3 py-4">
              <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-default">
                <Mark className="size-4 text-brand-accent" />
              </span>
              <div className="min-w-0 flex-1">
                <h2 className="text-xs font-medium">{entry.name}</h2>
                <p className="mt-1 text-chrome leading-5 text-muted">{entry.description}</p>
              </div>
              <Switch
                aria-label={`Enable ${entry.name}`}
                isSelected={entry.enabled}
                isDisabled={entry.required}
                onChange={() => {
                  togglePlugin(entry.id);
                }}
              >
                <Switch.Content>
                  <Switch.Control>
                    <Switch.Thumb />
                  </Switch.Control>
                </Switch.Content>
              </Switch>
              <IconButton
                icon={Settings}
                label={`Configure ${entry.name}`}
                onPress={() => {
                  setDetail(entry.id);
                }}
              />
            </div>
          );
        })}
      </div>
    </>
  );
}
