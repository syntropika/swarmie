import { useSource } from "@swarmie/client-ui";
import { ChromeButton } from "@swarmie/ui-web";
import { Copy } from "lucide-react";
import { useState } from "react";

import { useClient } from "../client/context";
import { IconButton, copyText } from "../controls";

export function JsonValue({
  name,
  value,
  expanded,
  depth = 0,
}: {
  name?: string;
  value: unknown;
  expanded: boolean;
  depth?: number;
}) {
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    return (
      <details open={expanded || depth === 0} className="pl-3">
        <summary className="min-h-(--spacing-chrome-menu-row) cursor-pointer py-0.5 text-foreground">
          <span className="text-brand-accent-text">{name}</span>
          {name === undefined ? "" : ": "}
          {Array.isArray(value) ? "[" : "{"}
          <span className="ml-2 text-muted">
            {entries.length} {Array.isArray(value) ? "items" : "keys"}
          </span>
        </summary>
        <div className="border-l border-border">
          {entries.map(([key, item]) => (
            <JsonValue key={key} name={key} value={item} expanded={expanded} depth={depth + 1} />
          ))}
        </div>
        <span>{Array.isArray(value) ? "]" : "}"}</span>
      </details>
    );
  }
  return (
    <div className="pl-6 leading-6">
      <span className="text-muted">{name}</span>:{" "}
      <span className={typeof value === "string" ? "text-brand-accent-text" : "text-foreground"}>
        {JSON.stringify(value)}
      </span>
    </div>
  );
}
export function StateView() {
  const client = useClient();
  const layout = useSource(client.shell);
  const pluginList = useSource(client.plugins);
  const connections = useSource(client.connectionState);
  const conversations = useSource(client.conversationState);
  const providers = useSource(client.providers.state);
  const projects = useSource(client.projectsState);
  const settings = useSource(client.settings);
  const onboarding = useSource(client.onboarding);
  const activity = useSource(client.activity);
  const actions = useSource(client.conversationsUi.composer.actions);
  const slash = useSource(client.conversationsUi.composer.slashCommands);
  const messageActions = useSource(client.conversationsUi.messages.actions);
  const [expanded, setExpanded] = useState(false);
  const [revision, setRevision] = useState(0);
  const states = new Map<string, unknown>(
    Object.entries({
      shell: layout,
      connections,
      settings,
      conversations: { selected: layout.active, ...conversations },
      views: { tabs: layout.tabs, selected: layout.tab, expanded: layout.expanded },
      projects,
      providers: {
        ready: providers.ready,
        accounts: providers.accounts.map((account) => ({ ...account })),
        error: providers.error,
      },
      onboarding,
      activity,
      "read-aloud": {
        language: settings.language,
        composerActions: actions.map((action) => action.id),
        messageActions: messageActions.map((action) => action.id),
      },
      "app-state": { available: true },
    }),
  );
  const value = {
    connections,
    shell: layout,
    plugins: Object.fromEntries(
      pluginList.map((plugin) => [
        plugin.id,
        {
          activation: plugin.enabled ? "active" : "disabled",
          errors: plugin.errors,
          state: states.get(plugin.id) ?? {},
        },
      ]),
    ),
    extensionPoints: {
      composer: actions.map((action) => action.id),
      slash: slash.map((command) => command.id),
    },
  };
  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-(--spacing-navigation-header) shrink-0 flex-wrap items-center justify-between gap-x-2 border-b border-navigation-divider px-4 text-chrome">
        <span className="flex items-center gap-2">
          App state
          <span
            title="Subscribed: this view updates when plugin state changes."
            className="size-1.5 rounded-full bg-brand-accent"
          />
        </span>
        <div className="flex gap-1">
          <ChromeButton
            size="action"
            onPress={() => {
              setExpanded(true);
              setRevision((current) => current + 1);
            }}
          >
            Expand all
          </ChromeButton>
          <ChromeButton
            size="action"
            onPress={() => {
              setExpanded(false);
              setRevision((current) => current + 1);
            }}
          >
            Collapse all
          </ChromeButton>
          <IconButton
            icon={Copy}
            label="Copy app state"
            onPress={() => {
              copyText(JSON.stringify(value, null, 2));
            }}
          />
        </div>
      </div>
      <div key={revision} className="min-h-0 flex-1 overflow-auto p-4 font-mono text-chrome">
        <JsonValue value={value} expanded={expanded} />
      </div>
    </div>
  );
}
