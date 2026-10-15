import { useEffect, useState, useCallback, useId } from "react";

import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

import { useExtensionQuery } from "./components/extension-query";
import { extensionResult } from "./components/extension-result";
import { hasCount, hasText } from "./components/presentation-values";
import type { Thread } from "./protocol";
import type { WorkspaceStore } from "./workspace";

const resourceOwners = new WeakMap<WorkspaceStore, number>();
let nextResourceOwner = 0;
function resourceOwner(store: WorkspaceStore) {
  let owner = resourceOwners.get(store);
  if (owner === undefined) {
    nextResourceOwner += 1;
    owner = nextResourceOwner;
    resourceOwners.set(store, owner);
  }
  return owner;
}
export type Template = {
  id: string;
  name: string;
  text: string;
  aliases?: string[];
};
export function templateParameterNames(text: string): string[] {
  return [
    ...new Set(
      [...text.matchAll(/\{\{\s*(?<parameter>[\w.-]+)\s*\}\}/gu)].map(
        (match) => match.groups?.parameter ?? "",
      ),
    ),
  ];
}
export function parseTemplateAliases(text: string): string[] {
  const aliases = text
    .split(/[\s,]+/u)
    .filter(Boolean)
    .map((alias) => alias.replace(/^\//u, ""));
  if (
    aliases.length > 16 ||
    aliases.some((alias) => !/^[A-Za-z0-9_.-]{1,128}$/u.test(alias)) ||
    new Set(aliases).size !== aliases.length
  ) {
    throw new Error(
      "Use up to 16 unique aliases containing letters, digits, dots, hyphens or underscores.",
    );
  }
  return aliases;
}
export function matchingTemplateIdentifier(template: Template, query: string): string | undefined {
  const identifiers = [template.id, ...(template.aliases ?? [])];
  const lower = query.toLowerCase();
  return (
    identifiers.find((id) => id.toLowerCase() === lower) ??
    identifiers.find((id) => id.toLowerCase().includes(lower)) ??
    (template.name.toLowerCase().includes(lower) ? template.id : undefined)
  );
}
function TemplateParameters({
  template,
  values,
  setValues,
}: {
  template: Template;
  values: Record<string, string>;
  setValues: (values: Record<string, string>) => void;
}) {
  const fieldPrefix = useId();

  return (
    <div className="flex flex-col gap-2" aria-label={`Parameters for ${template.name}`}>
      {templateParameterNames(template.text).map((key) => (
        <label htmlFor={`${fieldPrefix}-field-1-${key}`} key={key}>
          {key}
          <Input
            id={`${fieldPrefix}-field-1-${key}`}
            value={values[key] ?? ""}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.stopPropagation();
              }
            }}
            onChange={(event) => {
              setValues({ ...values, [key]: event.target.value });
            }}
          />
        </label>
      ))}
    </div>
  );
}
export type NativeSkillCommand = {
  name: string;
  path?: string;
  description?: string;
  source?: string;
  scope?: string;
  invocation?: string;
  availability?: string;
  enabled?: boolean;
  userInvocable?: boolean;
};

function selectNativeSkill(store: WorkspaceStore, thread: Thread, command: NativeSkillCommand) {
  if (
    command.source !== "skill" ||
    !hasText(command.path) ||
    command.enabled === false ||
    command.userInvocable === false ||
    (hasText(command.availability) && command.availability !== "advertised") ||
    thread.capabilities?.nativeSkillInputs?.availability !== "available"
  ) {
    return;
  }
  const skill = { name: command.name.replace(/^[/$]/u, ""), path: command.path };
  const current = store.getSnapshot().threads.find((item) => item.id === thread.id);
  if (!current) {
    return;
  }
  const multiple = thread.harness === "codex";
  store.updateThread(thread.id, {
    contextItems: [
      ...(current.contextItems ?? []).filter(
        (item) => !item.skill || (multiple && item.skill.path !== skill.path),
      ),
      {
        id: crypto.randomUUID(),
        kind: "resource",
        label: `Skill: ${skill.name}`,
        text: `Native skill selected from ${skill.path}`,
        path: skill.path,
        skill,
      },
    ],
  });
}

type Resource = {
  id: string;
  name: string;
  kind: string;
  path: string;
  source: string;
  native: boolean;
  availability?: string;
  invocation?: string;
  userInvocable?: boolean;
  selection?: { name: string; path: string };
  conflicts?: string[];
  diagnostics?: string[];
};
export function ResourceLibrary(props: { store: WorkspaceStore; thread: Thread }) {
  const { store, thread } = props;
  const key = JSON.stringify([
    resourceOwner(store),
    thread.id,
    thread.worktreeId,
    thread.sessionId,
    thread.harness,
    thread.commandsRevision,
  ]);
  return <ResourceLibraryContent key={key} {...props} />;
}

function ResourcePreview({
  read,
  store,
  thread,
}: {
  read: { path: string; text: string; resource: Resource };
  store: WorkspaceStore;
  thread: Thread;
}) {
  return (
    <Disclosure defaultOpen>
      <Disclosure.Summary>{read.path}</Disclosure.Summary>
      <pre className="action-output">{read.text}</pre>
      {read.resource.diagnostics?.map((message) => (
        <p key={message} className="panel-note">
          {message}
        </p>
      ))}
      {(read.resource.conflicts?.length ?? 0) > 0 && (
        <p className="panel-note">
          Other skills share this name. Inspect their paths before choosing a native skill.
        </p>
      )}
      {read.resource.kind === "skill" && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={
            read.resource.availability !== "advertised" ||
            read.resource.userInvocable === false ||
            thread.capabilities?.nativeSkillInputs?.availability !== "available"
          }
          onClick={() => {
            selectNativeSkill(store, thread, {
              ...read.resource,
              ...read.resource.selection,
              source: "skill",
            });
            const current = store.getSnapshot().threads.find((item) => item.id === thread.id);
            if (current && !hasText(current.draft?.trim())) {
              store.updateThread(thread.id, {
                draft: read.resource.invocation ?? `$${read.resource.name} `,
              });
            }
            window.dispatchEvent(
              new CustomEvent("swarmie:composer-text", { detail: { threadId: thread.id } }),
            );
          }}
        >
          Use native skill
        </Button>
      )}
      <Button
        size="sm"
        type="button"
        variant="ghost"

        onClick={() => {
          try {
            store.addContext(thread.id, {
              kind: "resource",
              label: read.path.split("/").at(-1) ?? read.path,
              text: read.text,
              path: read.path,
            });
          } catch (error) {
            store.setError(error);
          }
        }}
      >
        Attach instructions as text
      </Button>
    </Disclosure>
  );
}

function ResourceLibraryContent({ store, thread }: { store: WorkspaceStore; thread: Thread }) {
  const fieldPrefix = useId();

  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [aliases, setAliases] = useState("");
  const [pendingTemplate, setPendingTemplate] = useState<Template>();
  const [variables, setVariables] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<Template>();
  const [read, setRead] = useState<{
    path: string;
    text: string;
    resource: Resource;
    owner: WorkspaceStore;
    scope: string;
  }>();
  const [query, setQuery] = useState("");
  const [history, setHistory] = useState<
    { sessionId: string; sequence: number; text: string; timestampMs: number }[]
  >([]);
  const [busy, setBusy] = useState(false);
  const run = useCallback(
    async (action: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await action();
      } catch (error) {
        store.setError(error);
      } finally {
        setBusy(false);
      }
    },
    [store],
  );
  const fetchCatalog = useCallback(async () => {
    const [saved, found] = await Promise.all([
      store.invokeExtension<{ templates: Template[] }>(
        "resources",
        "templates.list",
        {},
        extensionResult<{ templates: Template[] }>({
          type: "object",
          properties: {
            templates: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  id: { type: "string" },
                  name: { type: "string" },
                  text: { type: "string" },
                  aliases: { type: "array", items: { type: "string" } },
                },
                required: ["id", "name", "text"],
              },
            },
          },
          required: ["templates"],
        }),
      ),
      store.invokeExtension<{ resources: Resource[]; limitations: string[] }>(
        "resources",
        "resources.list",
        {
          cwd: thread.worktreeId,
          sessionId: thread.sessionId,
          harness: thread.harness,
        },
        extensionResult<{ resources: Resource[]; limitations: string[] }>({
          type: "object",
          properties: {
            resources: {
              type: "array",
              items: {
                type: "object",
                properties: {
                  id: { type: "string" },
                  name: { type: "string" },
                  kind: { type: "string" },
                  path: { type: "string" },
                  source: { type: "string" },
                  native: { type: "boolean" },
                },
                required: ["id", "name", "kind", "path", "source", "native"],
              },
            },
            limitations: { type: "array", items: { type: "string" } },
          },
          required: ["resources", "limitations"],
        }),
      ),
    ]);
    return {
      templates: saved.templates,
      resources: found.resources,
      limitations: found.limitations,
    };
  }, [store, thread.harness, thread.worktreeId, thread.sessionId]);
  const catalog = useExtensionQuery(
    store,
    [
      "resource-catalog",
      resourceOwner(store),
      thread.id,
      thread.harness,
      thread.worktreeId,
      thread.sessionId,
      thread.commandsRevision,
    ],
    fetchCatalog,
  );
  const templates = catalog.data?.templates ?? [];
  const resources = catalog.data?.resources ?? [];
  const limitations = catalog.data?.limitations ?? [];
  const reload = catalog.refresh;
  const insertTemplate = async (template: Template, templateValues: Record<string, string>) => {
    const rendered = await store.invokeExtension<{ text: string }>(
      "resources",
      "templates.render",
      { id: template.id, variables: templateValues },
      extensionResult<{ text: string }>({
        type: "object",
        properties: { text: { type: "string" } },
        required: ["text"],
      }),
    );
    const current = store.getSnapshot().threads.find((item) => item.id === thread.id);
    store.updateThread(thread.id, {
      draft: `${current?.draft ?? ""}${hasText(current?.draft) ? "\n" : ""}${rendered.text}`,
    });
    setPendingTemplate(undefined);
    window.dispatchEvent(
      new CustomEvent("swarmie:composer-text", {
        detail: { threadId: thread.id },
      }),
    );
  };
  const chooseTemplate = async (template: Template) => {
    const names = templateParameterNames(template.text);
    if (names.length === 0) {
      await insertTemplate(template, {});
      return;
    }
    setVariables(Object.fromEntries(names.map((key) => [key, ""])));
    setPendingTemplate(template);
  };
  return (
    <div className="panel-body">
      <div className="review-toolbar">
        <h3>Skills and templates</h3>
        <Button
          size="sm"
          type="button"
          variant="ghost"

          disabled={busy}
          onClick={() => {
            run(reload).catch(store.setError.bind(store));
          }}
        >
          Reload resources
        </Button>
      </div>
      <Input
        aria-label="Search skills and templates"
        placeholder="Search resources…"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
        }}
      />
      {resources
        .filter((r) => r.name.toLowerCase().includes(query.toLowerCase()))
        .map((r) => (
          <Button
            type="button"
            key={r.id}
            variant="command"

            title={r.path}
            onClick={() => {
              run(async () => {
                const value = await store.invokeExtension(
                  "resources",
                  "resources.read",
                  {
                    cwd: thread.worktreeId,
                    sessionId: thread.sessionId,
                    path: r.path,
                  },
                  extensionResult<{ path: string; text: string }>({
                    type: "object",
                    properties: { path: { type: "string" }, text: { type: "string" } },
                    required: ["path", "text"],
                  }),
                );
                setRead({ ...value, resource: r, owner: store, scope: "" });
              }).catch(store.setError.bind(store));
            }}
          >
            {r.name}
            <span className="muted">
              {r.kind} · {r.source} · {r.availability ?? "discovered"}
            </span>
          </Button>
        ))}
      {read !== undefined && <ResourcePreview read={read} store={store} thread={thread} />}
      {limitations.map((message) => (
        <p key={message} className="panel-note">
          {message}
        </p>
      ))}
      <h3>Saved templates</h3>
      {templates
        .filter((t) => matchingTemplateIdentifier(t, query) !== undefined)
        .map((template) => (
          <div className="resource-row" key={template.id}>
            <Button
              type="button"
              variant="command"

              onClick={() => {
                run(async () => {
                  await chooseTemplate(template);
                }).catch(store.setError.bind(store));
              }}
            >
              {template.name}
              {hasCount(template.aliases?.length) && (
                <span className="muted">
                  {template.aliases.map((alias) => `/${alias}`).join(" · ")}
                </span>
              )}
            </Button>
            <Button
              size="sm"
              type="button"
              variant="ghost"

              onClick={() => {
                setSelected(template);
                setName(template.name);
                setText(template.text);
                setAliases((template.aliases ?? []).join(", "));
              }}
            >
              Edit
            </Button>
          </div>
        ))}
      {pendingTemplate && (
        <div className="permission-card">
          <strong>{pendingTemplate.name}</strong>
          <TemplateParameters
            template={pendingTemplate}
            values={variables}
            setValues={setVariables}
          />
          <Button
            size="sm"
            type="button"
            variant="ghost"

            disabled={busy}
            onClick={() => {
              run(async () => {
                await insertTemplate(pendingTemplate, variables);
              }).catch(store.setError.bind(store));
            }}
          >
            Insert template
          </Button>
          <Button
            size="sm"
            type="button"
            variant="ghost"

            onClick={() => {
              setPendingTemplate(undefined);
            }}
          >
            Cancel
          </Button>
        </div>
      )}
      <Disclosure>
        <Disclosure.Summary>
          {selected ? `Edit ${selected.name}` : "New prompt template"}
        </Disclosure.Summary>
        <label htmlFor={`${fieldPrefix}-field-2`}>
          Name
          <Input
            id={`${fieldPrefix}-field-2`}
            value={name}
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </label>
        <label htmlFor={`${fieldPrefix}-field-3`}>
          Slash aliases
          <Input
            id={`${fieldPrefix}-field-3`}
            value={aliases}
            onChange={(event) => {
              setAliases(event.target.value);
            }}
            placeholder="review, inspect"
          />
        </label>
        <label htmlFor={`${fieldPrefix}-field-4`}>
          Prompt
          <Textarea
            id={`${fieldPrefix}-field-4`}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
            placeholder="Use {{variable}} for required parameters."
          />
        </label>
        <div className="permission-actions">
          <Button
            size="sm"
            type="button"
            variant="ghost"

            disabled={busy || !name.trim() || !text.trim()}
            onClick={() => {
              run(async () => {
                await store.invokeExtension("resources", "templates.put", {
                  id: selected?.id ?? crypto.randomUUID(),
                  name,
                  text,
                  aliases: parseTemplateAliases(aliases),
                });
                setSelected(undefined);
                setName("");
                setText("");
                setAliases("");
                window.dispatchEvent(new CustomEvent("swarmie:templates"));
                await reload();
              }).catch(store.setError.bind(store));
            }}
          >
            Save template
          </Button>
          <Button
            size="sm"
            type="button"
            variant="ghost"

            onClick={() => {
              setName(`${thread.title} prompt`);
              setText(thread.draft ?? "");
              setSelected(undefined);
              setAliases("");
            }}
          >
            Use current draft
          </Button>
          {selected && (
            <Button
              size="sm"
              type="button"
              variant="destructive"

              onClick={() => {
                run(async () => {
                  await store.invokeExtension("resources", "templates.remove", {
                    id: selected.id,
                  });
                  setSelected(undefined);
                  setName("");
                  setText("");
                  setAliases("");
                  setPendingTemplate(undefined);
                  window.dispatchEvent(new CustomEvent("swarmie:templates"));
                  await reload();
                }).catch(store.setError.bind(store));
              }}
            >
              Remove template
            </Button>
          )}
        </div>
      </Disclosure>
      <Disclosure
        onOpenChange={(open) => {
          if (open) {
            run(async () => {
              const result = await store.invokeExtension<{
                items: typeof history;
              }>(
                "resources",
                "history.list",
                {
                  cwd: thread.worktreeId,
                  sessionId: thread.sessionId,
                  query,
                  limit: 40,
                },
                extensionResult<{
                  items: typeof history;
                }>({
                  type: "object",
                  properties: {
                    items: {
                      type: "array",
                      items: {
                        type: "object",
                        properties: {
                          sessionId: { type: "string" },
                          sequence: { type: "number" },
                          text: { type: "string" },
                          timestampMs: { type: "number" },
                        },
                        required: ["sessionId", "sequence", "text", "timestampMs"],
                      },
                    },
                  },
                  required: ["items"],
                }),
              );
              setHistory(result.items);
            }).catch(store.setError.bind(store));
          }
        }}
      >
        <Disclosure.Summary>Prompt history</Disclosure.Summary>
        {history.map((item) => (
          <Button
            type="button"
            key={`${item.sessionId}:${item.sequence}`}
            variant="command"

            onClick={() => {
              store.updateThread(thread.id, { draft: item.text });
              window.dispatchEvent(
                new CustomEvent("swarmie:composer-text", {
                  detail: { threadId: thread.id },
                }),
              );
            }}
          >
            {item.text.slice(0, 140)}
            <span className="muted">{new Date(item.timestampMs).toLocaleString()}</span>
          </Button>
        ))}
        {!history.length && <p className="panel-note">No recorded prompts for this project.</p>}
      </Disclosure>
    </div>
  );
}

function SlashCommandsContent({
  thread,
  store,
  setText,
}: {
  thread: Thread;
  store: WorkspaceStore;
  setText: (text: string) => void;
}) {
  const [commands, setCommands] = useState<NativeSkillCommand[]>([]);
  const scope = JSON.stringify([
    thread.id,
    thread.worktreeId,
    thread.sessionId,
    thread.harness,
    thread.commandsRevision,
  ]);
  const [catalogOwner, setCatalogOwner] = useState<{ store: WorkspaceStore; scope: string }>();
  const catalogCurrent = catalogOwner?.store === store && catalogOwner.scope === scope;
  const [templates, setTemplates] = useState<Template[]>([]);
  const [pendingSelection, setPendingSelection] = useState<{
    template: Template;
    identifier: string;
    scope: string;
  }>();
  const [variables, setVariables] = useState<Record<string, string>>({});
  const [rendering, setRendering] = useState(false);
  const query = /^\/(?<command>[^\s]*)$/u.exec(thread.draft ?? "")?.groups?.command;
  const queryEnabled = query !== undefined;
  const selectionScope = `${thread.id}:${query ?? ""}`;
  const pendingTemplate = pendingSelection?.scope === selectionScope ? pendingSelection : undefined;
  useEffect(() => {
    if (!queryEnabled) {
      return undefined;
    }
    let cancelled = false;
    let revision = 0;

    const load = async () => {
      revision += 1;
      const current = revision;
      if (store.getSnapshot().extensions.some((e) => e.id === "resources")) {
        const result = await store.invokeExtension<{ templates: Template[] }>(
          "resources",
          "templates.list",
          {},
          extensionResult<{ templates: Template[] }>({
            type: "object",
            properties: {
              templates: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    id: { type: "string" },
                    name: { type: "string" },
                    text: { type: "string" },
                    aliases: { type: "array", items: { type: "string" } },
                  },
                  required: ["id", "name", "text"],
                },
              },
            },
            required: ["templates"],
          }),
        );
        if (!cancelled && current === revision) {
          setTemplates(result.templates);
          setCatalogOwner({ store, scope });
        }
      }
      if (hasText(thread.sessionId)) {
        const rawCatalog = await store.rpc("agent/commands", { sessionId: thread.sessionId });
        const catalog = extensionResult<NativeSkillCommand[]>({
          type: "array",
          items: {
            type: "object",
            properties: {
              name: { type: "string" },
              description: { type: "string" },
              source: { type: "string" },
              path: { type: "string" },
              invocation: { type: "string" },
            },
            required: ["name"],
          },
        })(rawCatalog);
        if (!cancelled && current === revision) {
          setCommands(catalog);
          setCatalogOwner({ store, scope });
        }
      }
    };
    const reportRefreshFailure = (failure: unknown) => {
      if (!cancelled) {
        store.setError(failure);
      }
    };
    const refresh = () => {
      load().catch(reportRefreshFailure);
    };
    refresh();
    window.addEventListener("swarmie:templates", refresh);
    return () => {
      cancelled = true;
      window.removeEventListener("swarmie:templates", refresh);
    };
  }, [queryEnabled, scope, store, thread.sessionId]);

  if (query === undefined) {
    return null;
  }
  const choose = (text: string) => {
    store.updateThread(thread.id, { draft: text });
    setText(text);
  };
  const render = async (identifier: string, values: Record<string, string>) => {
    setRendering(true);
    try {
      const result = await store.invokeExtension<{ text: string }>(
        "resources",
        "templates.render",
        { id: identifier, variables: values },
        extensionResult<{ text: string }>({
          type: "object",
          properties: { text: { type: "string" } },
          required: ["text"],
        }),
      );
      choose(result.text);
      setPendingSelection(undefined);
    } catch (error) {
      store.setError(error);
    } finally {
      setRendering(false);
    }
  };
  return (
    <div className="composer-suggestions" aria-label="Prompt commands">
      {pendingTemplate && (
        <div className="permission-card">
          <strong>{pendingTemplate.template.name}</strong>
          <TemplateParameters
            template={pendingTemplate.template}
            values={variables}
            setValues={setVariables}
          />
          <Button
            size="sm"
            type="button"
            variant="ghost"

            disabled={rendering}
            onClick={() => {
              render(pendingTemplate.identifier, variables).catch(store.setError.bind(store));
            }}
          >
            Apply template
          </Button>
          <Button
            size="sm"
            type="button"
            variant="ghost"

            onClick={() => {
              setPendingSelection(undefined);
            }}
          >
            Cancel
          </Button>
        </div>
      )}
      {(catalogCurrent ? commands : [])
        .filter(
          (c) =>
            c.name.toLowerCase().includes(query.toLowerCase()) &&
            c.enabled !== false &&
            c.userInvocable !== false &&
            c.availability !== "requires-restart",
        )
        .slice(0, 8)
        .map((command) => (
          <Button
            type="button"
            key={`${command.name}:${command.path ?? command.source}`}
            variant="command"
            title={command.path ?? command.availability}
            disabled={
              command.source === "skill" &&
              (!hasText(command.path) ||
                thread.capabilities?.nativeSkillInputs?.availability !== "available" ||
                (command.availability !== undefined && command.availability !== "advertised"))
            }
            onClick={() => {
              selectNativeSkill(store, thread, command);
              choose(command.invocation ?? `/${command.name.replace(/^[/$]/u, "")} `);
            }}
          >
            {command.source === "skill"
              ? `$${command.name}`
              : (command.invocation ?? `/${command.name.replace(/^[/$]/u, "")}`).trim()}
            <span className="muted">
              {command.source === "skill"
                ? `Native skill · ${command.scope ?? command.path ?? "session"}`
                : "Native command"}
            </span>
          </Button>
        ))}
      {(catalogCurrent ? templates : [])
        .filter((t) => matchingTemplateIdentifier(t, query) !== undefined)
        .slice(0, 8)
        .map((template) => (
          <Button
            type="button"
            key={template.id}
            variant="command"

            disabled={rendering}
            onClick={() => {
              const identifier = matchingTemplateIdentifier(template, query);
              if (identifier === undefined) {
                return;
              }
              const names = templateParameterNames(template.text);
              if (names.length) {
                setVariables(Object.fromEntries(names.map((key) => [key, ""])));
                setPendingSelection({ template, identifier, scope: selectionScope });
              } else {
                render(identifier, {}).catch(store.setError.bind(store));
              }
            }}
          >
            {template.name}
            <span className="muted">
              Template
              {hasCount(template.aliases?.length)
                ? ` · ${template.aliases.map((alias) => `/${alias}`).join(" · ")}`
                : ""}
            </span>
          </Button>
        ))}
    </div>
  );
}

export function SlashCommands(props: {
  thread: Thread;
  store: WorkspaceStore;
  setText: (text: string) => void;
}) {
  return (
    <SlashCommandsContent key={`${props.thread.id}:${props.thread.sessionId ?? ""}`} {...props} />
  );
}
