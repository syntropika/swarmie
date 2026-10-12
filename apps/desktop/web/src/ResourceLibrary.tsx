import { Disclosure } from "@/components/ui/disclosure";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { WorkspaceStore } from "./workspace";
import type { Thread } from "./protocol";
export type Template = {
  id: string;
  name: string;
  text: string;
  aliases?: string[];
};
export function templateParameterNames(text: string): string[] {
  return [
    ...new Set(
      [...text.matchAll(/\{\{\s*([\w.-]+)\s*\}\}/g)].map((match) => match[1]),
    ),
  ];
}
export function parseTemplateAliases(text: string): string[] {
  const aliases = text
    .split(/[\s,]+/)
    .filter(Boolean)
    .map((alias) => alias.replace(/^\//, ""));
  if (
    aliases.length > 16 ||
    aliases.some((alias) => !/^[A-Za-z0-9_.-]{1,128}$/.test(alias)) ||
    new Set(aliases).size !== aliases.length
  )
    throw new Error(
      "Use up to 16 unique aliases containing letters, digits, dots, hyphens or underscores.",
    );
  return aliases;
}
export function matchingTemplateIdentifier(
  template: Template,
  query: string,
): string | undefined {
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
  return (
    <div
      className="template-parameters"
      aria-label={`Parameters for ${template.name}`}
    >
      {templateParameterNames(template.text).map((key) => (
        <label key={key}>
          {key}
          <Input
            value={values[key] ?? ""}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                event.stopPropagation();
              }
            }}
            onChange={(event) =>
              setValues({ ...values, [key]: event.target.value })
            }
          />
        </label>
      ))}
    </div>
  );
}
type Resource = {
  id: string;
  name: string;
  kind: string;
  path: string;
  source: string;
  native: boolean;
};
export function ResourceLibrary({
  store,
  thread,
}: {
  store: WorkspaceStore;
  thread: Thread;
}) {
  const [templates, setTemplates] = useState<Template[]>([]);
  const [resources, setResources] = useState<Resource[]>([]);
  const [limitations, setLimitations] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [aliases, setAliases] = useState("");
  const [pendingTemplate, setPendingTemplate] = useState<Template>();
  const [variables, setVariables] = useState<Record<string, string>>({});
  const [selected, setSelected] = useState<Template>();
  const [read, setRead] = useState<{ path: string; text: string }>();
  const [query, setQuery] = useState("");
  const [history, setHistory] = useState<
    { sessionId: string; sequence: number; text: string; timestampMs: number }[]
  >([]);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (e) {
      store.setError(e);
    } finally {
      setBusy(false);
    }
  };
  const reload = async () => {
    const [saved, found] = await Promise.all([
      store.invokeExtension<{ templates: Template[] }>(
        "resources",
        "templates.list",
        {},
      ),
      store.invokeExtension<{ resources: Resource[]; limitations: string[] }>(
        "resources",
        "resources.list",
        {
          cwd: thread.worktreeId,
          sessionId: thread.sessionId,
          harness: thread.harness,
        },
      ),
    ]);
    setTemplates(saved.templates);
    setResources(found.resources);
    setLimitations(found.limitations);
  };
  useEffect(() => {
    void run(reload);
  }, [thread.worktreeId, thread.harness, thread.sessionId]);
  const insertTemplate = async (
    template: Template,
    variables: Record<string, string>,
  ) => {
    const rendered = await store.invokeExtension<{ text: string }>(
      "resources",
      "templates.render",
      { id: template.id, variables },
    );
    const current = store
      .getSnapshot()
      .threads.find((item) => item.id === thread.id);
    store.updateThread(thread.id, {
      draft: `${current?.draft ?? ""}${current?.draft ? "\n" : ""}${rendered.text}`,
    });
    setPendingTemplate(undefined);
    window.dispatchEvent(
      new CustomEvent("mpai:composer-text", {
        detail: { threadId: thread.id },
      }),
    );
  };
  const useTemplate = async (template: Template) => {
    const names = templateParameterNames(template.text);
    if (!names.length) return insertTemplate(template, {});
    setVariables(Object.fromEntries(names.map((key) => [key, ""])));
    setPendingTemplate(template);
  };
  return (
    <div className="panel-body">
      <div className="review-toolbar">
        <h3>Skills and templates</h3>
        <Button
          type="button"
          variant="ghost"
          className="small-button"
          disabled={busy}
          onClick={() => void run(reload)}
        >
          Reload resources
        </Button>
      </div>
      <Input
        aria-label="Search skills and templates"
        placeholder="Search resources…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {resources
        .filter((r) => r.name.toLowerCase().includes(query.toLowerCase()))
        .map((r) => (
          <Button
            type="button"
            key={r.id}
            variant="ghost"
            className="command-result"
            title={r.path}
            onClick={() =>
              void run(async () =>
                setRead(
                  await store.invokeExtension("resources", "resources.read", {
                    cwd: thread.worktreeId,
                    sessionId: thread.sessionId,
                    path: r.path,
                  }),
                ),
              )
            }
          >
            {r.name}
            <span className="muted">
              {r.kind} · {r.source}
            </span>
          </Button>
        ))}
      {read && (
        <Disclosure defaultOpen>
          <Disclosure.Summary>{read.path}</Disclosure.Summary>
          <pre className="action-output">{read.text}</pre>
          <Button
            type="button"
            variant="ghost"
            className="small-button"
            onClick={() => {
              try {
                store.addContext(thread.id, {
                  kind: "resource",
                  label: read.path.split("/").at(-1) ?? read.path,
                  text: read.text,
                  path: read.path,
                });
              } catch (e) {
                store.setError(e);
              }
            }}
          >
            Add resource to message
          </Button>
        </Disclosure>
      )}
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
              variant="ghost"
              className="command-result"
              onClick={() => void run(() => useTemplate(template))}
            >
              {template.name}
              {!!template.aliases?.length && (
                <span className="muted">
                  {template.aliases.map((alias) => `/${alias}`).join(" · ")}
                </span>
              )}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="small-button"
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
            type="button"
            variant="ghost"
            className="small-button"
            disabled={busy}
            onClick={() =>
              void run(() => insertTemplate(pendingTemplate, variables))
            }
          >
            Insert template
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="small-button"
            onClick={() => setPendingTemplate(undefined)}
          >
            Cancel
          </Button>
        </div>
      )}
      <Disclosure>
        <Disclosure.Summary>
          {selected ? `Edit ${selected.name}` : "New prompt template"}
        </Disclosure.Summary>
        <label>
          Name
          <Input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label>
          Slash aliases
          <Input
            value={aliases}
            onChange={(event) => setAliases(event.target.value)}
            placeholder="review, inspect"
          />
        </label>
        <label>
          Prompt
          <Textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Use {{variable}} for required parameters."
          />
        </label>
        <div className="permission-actions">
          <Button
            type="button"
            variant="ghost"
            className="small-button"
            disabled={busy || !name.trim() || !text.trim()}
            onClick={() =>
              void run(async () => {
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
                window.dispatchEvent(new CustomEvent("mpai:templates"));
                await reload();
              })
            }
          >
            Save template
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="small-button"
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
              type="button"
              variant="ghost"
              className="small-button danger-button"
              onClick={() =>
                void run(async () => {
                  await store.invokeExtension("resources", "templates.remove", {
                    id: selected.id,
                  });
                  setSelected(undefined);
                  setName("");
                  setText("");
                  setAliases("");
                  setPendingTemplate(undefined);
                  window.dispatchEvent(new CustomEvent("mpai:templates"));
                  await reload();
                })
              }
            >
              Remove template
            </Button>
          )}
        </div>
      </Disclosure>
      <Disclosure
        onOpenChange={(open) => {
          if (open)
            void run(async () => {
              const result = await store.invokeExtension<{
                items: typeof history;
              }>("resources", "history.list", {
                cwd: thread.worktreeId,
                sessionId: thread.sessionId,
                query,
                limit: 40,
              });
              setHistory(result.items);
            });
        }}
      >
        <Disclosure.Summary>Prompt history</Disclosure.Summary>
        {history.map((item) => (
          <Button
            type="button"
            key={`${item.sessionId}:${item.sequence}`}
            variant="ghost"
            className="command-result"
            onClick={() => {
              store.updateThread(thread.id, { draft: item.text });
              window.dispatchEvent(
                new CustomEvent("mpai:composer-text", {
                  detail: { threadId: thread.id },
                }),
              );
            }}
          >
            {item.text.slice(0, 140)}
            <span className="muted">
              {new Date(item.timestampMs).toLocaleString()}
            </span>
          </Button>
        ))}
        {!history.length && (
          <p className="panel-note">No recorded prompts for this project.</p>
        )}
      </Disclosure>
    </div>
  );
}

export function SlashCommands({
  thread,
  store,
  setText,
}: {
  thread: Thread;
  store: WorkspaceStore;
  setText: (text: string) => void;
}) {
  const [commands, setCommands] = useState<
    { name: string; description?: string; source?: string; path?: string; invocation?: string }[]
  >([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [pendingTemplate, setPendingTemplate] = useState<{
    template: Template;
    identifier: string;
  }>();
  const [variables, setVariables] = useState<Record<string, string>>({});
  const [rendering, setRendering] = useState(false);
  const query = /^\/([^\s]*)$/.exec(thread.draft ?? "")?.[1];
  useEffect(() => {
    if (query === undefined) return;
    let cancelled = false;
    let revision = 0;
    setCommands([]);
    const load = async () => {
      const current = ++revision;
      if (store.getSnapshot().extensions.some((e) => e.id === "resources")) {
        const result = await store.invokeExtension<{ templates: Template[] }>(
          "resources",
          "templates.list",
          {},
        );
        if (!cancelled && current === revision) setTemplates(result.templates);
      }
      if (thread.sessionId) {
        const catalog = await store.rpc<
          { name: string; description?: string; source?: string }[]
        >("agent/commands", { sessionId: thread.sessionId });
        if (!cancelled && current === revision) setCommands(catalog);
      }
    };
    const refresh = () =>
      void load().catch((e) => {
        if (!cancelled) store.setError(e);
      });
    refresh();
    window.addEventListener("mpai:templates", refresh);
    return () => {
      cancelled = true;
      window.removeEventListener("mpai:templates", refresh);
    };
  }, [
    query !== undefined,
    thread.sessionId,
    thread.commandsRevision,
    thread.id,
  ]);
  useEffect(() => {
    setPendingTemplate(undefined);
  }, [query, thread.id]);
  if (query === undefined) return null;
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
      );
      choose(result.text);
      setPendingTemplate(undefined);
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
            type="button"
            variant="ghost"
            className="small-button"
            disabled={rendering}
            onClick={() => void render(pendingTemplate.identifier, variables)}
          >
            Apply template
          </Button>
          <Button
            type="button"
            variant="ghost"
            className="small-button"
            onClick={() => setPendingTemplate(undefined)}
          >
            Cancel
          </Button>
        </div>
      )}
      {commands
        .filter((c) => c.name.toLowerCase().includes(query.toLowerCase()))
        .slice(0, 8)
        .map((command) => (
          <Button
            type="button"
            key={`${command.name}:${command.path ?? command.source}`}
            variant="ghost"
            className="command-result"
            title={command.path}
            onClick={() => {
              if (
                command.source === "skill" &&
                command.path &&
                thread.capabilities?.nativeSkillInputs?.availability === "available"
              ) {
                const skill = {
                  name: command.name.replace(/^[/$]/, ""),
                  path: command.path,
                };
                const current = store
                  .getSnapshot()
                  .threads.find((item) => item.id === thread.id)!;
                store.updateThread(thread.id, {
                  contextItems: [
                    ...(current.contextItems ?? []).filter(
                      (item) => item.skill?.path !== skill.path,
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
              choose(command.invocation ?? `/${command.name.replace(/^[/$]/, "")} `);
            }}
          >
            {(command.invocation ?? `/${command.name.replace(/^[/$]/, "")}`).trim()}
            <span className="muted">
              {command.source === "skill" ? "Native skill" : "Native command"}
            </span>
          </Button>
        ))}
      {templates
        .filter((t) => matchingTemplateIdentifier(t, query) !== undefined)
        .slice(0, 8)
        .map((template) => (
          <Button
            type="button"
            key={template.id}
            variant="ghost"
            className="command-result"
            disabled={rendering}
            onClick={() => {
              const identifier = matchingTemplateIdentifier(template, query)!;
              const names = templateParameterNames(template.text);
              if (names.length) {
                setVariables(Object.fromEntries(names.map((key) => [key, ""])));
                setPendingTemplate({ template, identifier });
              } else void render(identifier, {});
            }}
          >
            {template.name}
            <span className="muted">
              Template
              {template.aliases?.length
                ? ` · ${template.aliases.map((alias) => `/${alias}`).join(" · ")}`
                : ""}
            </span>
          </Button>
        ))}
    </div>
  );
}
