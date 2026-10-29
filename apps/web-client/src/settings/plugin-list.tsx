import { useSource } from "@swarmie/client-ui";
import { ActionRow, Description, Switch } from "@swarmie/ui-web";
import { Bell, ChevronRight, Code2, Folder, Globe, Mic, Puzzle } from "lucide-react";

import { useClient } from "../client/context";
import { IconButton } from "../controls";

const icons = {
  projects: Folder,
  notifications: Bell,
  "read-aloud": Mic,
  views: Globe,
  "app-state": Code2,
  providers: Puzzle,
};
export function PluginList({ query, open }: { query: string; open: (id: string) => void }) {
  const { plugins, togglePlugin } = useClient();
  const list = useSource(plugins).filter((entry) =>
    `${entry.name} ${entry.description}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  if (list.length === 0) {
    return (
      <output className="block py-12 text-center text-sm text-muted">
        No plugins match “{query}”.
      </output>
    );
  }
  return (
    <div className="@container/plugin-list mt-2 space-y-6">
      {[
        { title: "Essentials", entries: list.filter((entry) => entry.required) },
        { title: "Plugins", entries: list.filter((entry) => !entry.required) },
      ]
        .filter((group) => group.entries.length > 0)
        .map((group) => (
          <section key={group.title} aria-label={group.title}>
            <h2 className="mb-3 text-caption font-medium text-muted">{group.title}</h2>
            <div className="divide-y divide-border-subtle overflow-hidden rounded-group bg-group">
              {group.entries.map((entry) => {
                const Mark = Object.entries(icons).find(([id]) => id === entry.id)?.[1] ?? Puzzle;
                return (
                  <ActionRow key={entry.id} title={entry.required ? "Required plugin" : undefined}>
                    <ActionRow.Trigger
                      onPress={() => {
                        open(entry.id);
                      }}
                    >
                      <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-default">
                        <Mark className="size-4 text-muted" />
                      </span>
                      <span className="flex min-w-0 flex-1 flex-col items-start gap-1">
                        <span className="wrap-anywhere">{entry.name}</span>
                        <Description className="wrap-anywhere whitespace-normal">
                          {entry.description}
                        </Description>
                      </span>
                    </ActionRow.Trigger>
                    {entry.required ? null : (
                      <Switch
                        aria-label={`Enable ${entry.name}`}
                        isSelected={entry.enabled}
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
                    )}
                    <span className="hidden @min-[384px]/plugin-list:contents">
                      <IconButton
                        icon={ChevronRight}
                        label={`Configure ${entry.name}`}
                        onPress={() => {
                          open(entry.id);
                        }}
                      />
                    </span>
                  </ActionRow>
                );
              })}
            </div>
          </section>
        ))}
    </div>
  );
}
