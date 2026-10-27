import { useSource } from "@swarmie/client-ui";
import { Switch } from "@swarmie/ui-web";
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
    <div className="mt-2 divide-y divide-border rounded-xl border border-border bg-chrome-surface px-4">
      {list.map((entry) => {
        const Mark = Object.entries(icons).find(([id]) => id === entry.id)?.[1] ?? Puzzle;
        return (
          <div key={entry.id} className="flex items-center gap-3 py-4">
            <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-default">
              <Mark className="size-4 text-muted" />
            </span>
            <button
              type="button"
              className="min-w-0 flex-1 text-left"
              onClick={() => {
                open(entry.id);
              }}
            >
              <span className="block text-sm font-medium">{entry.name}</span>
              <span className="mt-1 block text-xs leading-5 text-muted">{entry.description}</span>
            </button>
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
              icon={ChevronRight}
              label={`Configure ${entry.name}`}
              onPress={() => {
                open(entry.id);
              }}
            />
          </div>
        );
      })}
    </div>
  );
}
