import type { UiStore } from "@swarmie/client-ui";
import { selectStore } from "@swarmie/client-ui";
import type { SettingsState } from "@swarmie/plugin-browser/settings";
import type { ReferenceBase } from "@syntropika/tembra-sdk";

type OptionalPlugin = "projects" | "read-aloud" | "app-state" | "git";
type Activations = Record<OptionalPlugin, () => Promise<ReferenceBase>>;

function optional(id: string): id is OptionalPlugin {
  return id === "git" || id === "projects" || id === "read-aloud" || id === "app-state";
}

/** These controls hold consumer References to known, compiled frontend plugins. */
function corePermissions(id: string): string[] {
  if (id === "vault") {
    return [
      "Discover eligible secret references",
      "Receive credentials for approved execution",
      "Store and manage encrypted secrets",
    ];
  }
  if (id === "model-credentials") {
    return [
      "Request eligible model credentials",
      "Receive credentials for one approved model attempt",
    ];
  }
  if (id === "model-protection") {
    return ["Inspect all final content sent to a model"];
  }
  return [];
}

export function createPluginControls(
  settings: UiStore<Readonly<SettingsState>>,
  dev: boolean,
  activations: Activations,
  beforeDisable: (id: string) => void,
  failed: (error: unknown) => void,
) {
  const held = new Map<OptionalPlugin, ReferenceBase>();
  const pending = new Set<OptionalPlugin>();
  let closed = false;
  const plugins = selectStore(
    settings,
    (value) => value.plugins,
    (value, list) => ({ ...value, plugins: list }),
  );
  const base = [
    ["shell", "Application shell", "Navigation and application chrome.", true, []],
    ["connections", "Connections", "Access to execution nodes.", true, []],
    ["vault", "Vault", "Private credentials managed by your node.", true, []],
    [
      "model-protection",
      "Model protection",
      "Detect possible credentials before sending content to a model.",
      true,
      [],
    ],
    [
      "model-credentials",
      "Model credentials",
      "Resolve eligible credentials privately for model requests.",
      true,
      [],
    ],
    ["settings", "Settings", "Preferences and plugin configuration.", true, []],
    ["conversations", "Conversations", "Messages, history and an extensible composer.", true, []],
    [
      "projects",
      "Projects",
      "Organize conversations and personal folder links.",
      false,
      ["Conversations"],
    ],
    [
      "git",
      "Git",
      "Review files, changes and managed worktrees on available nodes.",
      false,
      ["Connections", "Views"],
    ],
    ["views", "Views", "Tabs and tools beside the conversation.", true, []],
    ["providers", "Providers", "Provider accounts and available models.", true, []],
    ["activity", "Activity", "The latest update from each conversation or plugin.", false, []],
    ["read-aloud", "Read aloud", "Browser speech and dictation.", false, ["Conversations"]],
    ...(dev
      ? [
          [
            "app-state",
            "App state",
            "Inspect the live state of frontend plugins.",
            false,
            [],
          ] as const,
        ]
      : []),
  ] as const;
  plugins.set(
    base.map(([id, name, description, required, uses]) => ({
      id,
      name,
      description,
      required,
      uses: [...uses],
      core: true,
      permissions: corePermissions(id),
      enabled: true,
      errors: [],
      version: "1",
      category: "Interface",
    })),
  );

  async function activate(id: OptionalPlugin) {
    if (closed || held.has(id) || pending.has(id)) {
      return;
    }
    pending.add(id);
    try {
      const reference = await activations[id]();
      if (closed || held.has(id)) {
        reference.release();
      } else {
        held.set(id, reference);
      }
    } finally {
      pending.delete(id);
    }
  }
  function togglePlugin(id: string) {
    const plugin = plugins.getSnapshot().find((entry) => entry.id === id);
    if (closed || !plugin || plugin.required || (optional(id) && pending.has(id))) {
      return;
    }
    const { enabled } = plugin;
    if (enabled) {
      beforeDisable(id);
      if (optional(id)) {
        held.get(id)?.release();
        held.delete(id);
      }
    }
    async function change() {
      if (!enabled && optional(id)) {
        await activate(id);
      }
      if (!closed) {
        plugins.update((list) =>
          list.map((entry) => (entry.id === id ? { ...entry, enabled: !enabled } : entry)),
        );
      }
    }
    change().catch(activationFailed);
  }
  function activationFailed(error: unknown) {
    if (!closed) {
      failed(error);
    }
  }
  function close() {
    closed = true;
    for (const reference of held.values()) {
      reference.release();
    }
    held.clear();
  }
  return { plugins, activate, togglePlugin, close };
}
