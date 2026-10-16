import type {
  McpCatalog,
  McpEntry,
  InputMcpMutation as McpMutation,
  InputMcpQuery as McpQuery,
} from "@swarmie/contracts";
import { Button, Input, Select, ListBox } from "@swarmie/ui-web";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronDown, Plus, RefreshCw } from "lucide-react";
import { isValidElement, useId, useState } from "react";

import { errorMessage } from "~/src/action-boundary";
import { McpServerEditor, editor, definition } from "~/src/mcp-server-editor";
import type { Editor } from "~/src/mcp-server-editor";
import { McpServerRow } from "~/src/mcp-server-row";
import { nodeKey } from "~/src/node-scope";
import { hasText } from "~/src/value-presence";
import type { Workspace, WorkspaceStore } from "~/src/workspace";

function savedMessage(operation: string, restartSessions: string[]) {
  if (restartSessions.length > 0) {
    return "Saved. Reopen the affected native sessions to apply this change.";
  }
  if (operation === "uninstall") {
    return "MCP server removed.";
  }
  if (operation === "authenticate") {
    return "Complete sign-in in your browser, then refresh the connection.";
  }
  return "MCP settings saved.";
}

function matchingEntries(catalog: McpCatalog | undefined, search: string) {
  return (
    catalog?.entries.filter((entry) =>
      `${entry.name} ${entry.origin} ${entry.harness ?? ""}`
        .toLowerCase()
        .includes(search.toLowerCase()),
    ) ?? []
  );
}

function openNativeSessions(state: Workspace) {
  return state.threads.filter(
    (thread) => hasText(thread.sessionId) && !["closed", "failed"].includes(thread.state),
  );
}

type SettingsProps = { store: WorkspaceStore; state: Workspace };
const owners = new WeakMap<WorkspaceStore, number>();
let nextOwner = 0;
function ownerKey(store: WorkspaceStore) {
  let owner = owners.get(store);
  if (owner === undefined) {
    nextOwner += 1;
    owner = nextOwner;
    owners.set(store, owner);
  }
  return owner;
}

export function McpSettings(props: SettingsProps) {
  const identity = props.state.node === undefined ? "" : nodeKey(props.state.node);
  return <McpContext key={`${ownerKey(props.store)}:${identity}`} {...props} />;
}

function McpContext(props: SettingsProps) {
  const [harness, setHarness] = useState("");
  const [sessionId, setSessionId] = useState("");
  const [cwd, setCwd] = useState("");
  const [directoryDraft, setDirectoryDraft] = useState("");
  const query: McpQuery =
    sessionId.length > 0
      ? { sessionId }
      : {
          ...(harness.length > 0 ? { harness } : {}),
          ...(cwd.trim().length > 0 ? { cwd: cwd.trim() } : {}),
        };
  const filters = {
    harness,
    setHarness,
    sessionId,
    setSessionId,
    cwd,
    setCwd,
    directoryDraft,
    setDirectoryDraft,
  };
  return <McpContent key={JSON.stringify(query)} {...props} query={query} filters={filters} />;
}

type Filters = {
  harness: string;
  setHarness: (value: string) => void;
  sessionId: string;
  setSessionId: (value: string) => void;
  cwd: string;
  setCwd: (value: string) => void;
  directoryDraft: string;
  setDirectoryDraft: (value: string) => void;
};
type Outcome = { notice?: string; authorization?: string | null };
type Operation = { key: string; work: () => Promise<Outcome | undefined> };

function McpContent({
  store,
  state,
  query,
  filters,
}: SettingsProps & { query: McpQuery; filters: Filters }) {
  const {
    harness,
    setHarness,
    sessionId,
    setSessionId,
    cwd,
    setCwd,
    directoryDraft,
    setDirectoryDraft,
  } = filters;
  const fieldPrefix = useId();
  const [search, setSearch] = useState("");
  const [edit, setEdit] = useState<Editor>();
  const [removing, setRemoving] = useState<McpEntry>();
  const catalog = useQuery({
    queryKey: ["mcp-catalog", ownerKey(store), state.node, query],
    queryFn: async () => await store.rpc("mcp/list", query),
    retry: false,
  });
  const action = useMutation({
    mutationFn: async (operation: Operation) => await operation.work(),
    retry: false,
  });
  let busy = catalog.isFetching ? "load" : "";
  if (action.isPending) {
    busy = action.variables.key;
  }
  const failure = action.error ?? catalog.error;
  const error = failure === null ? "" : errorMessage(failure);
  const { notice, authorization } = action.data ?? {};
  const run = (key: string, work: Operation["work"]) => {
    action.mutate({ key, work });
  };
  const reload = async () => {
    const result = await catalog.refetch();
    if (result.error !== null) {
      throw result.error;
    }
    return undefined;
  };
  const mutate = async (operation: string, entry?: McpEntry, form?: Editor): Promise<Outcome> => {
    const params: McpMutation = {
      query,
      action: operation,
      id: entry?.id,
      expectedRevision: entry?.revision,
      ...(form === undefined
        ? {}
        : {
            origin: form.origin,
            name: form.name.trim(),
            scope: form.scope,
            definition: definition(form),
          }),
    };
    const value = await store.rpc("mcp/mutate", params);
    setEdit(undefined);
    setRemoving(undefined);
    await catalog.refetch();
    return {
      notice: savedMessage(operation, value.restartSessions),
      authorization: value.authorizationUrl,
    };
  };
  const shown = catalog.data;
  const sessions = openNativeSessions(state);
  const entries = matchingEntries(shown, search);
  const selectedSession = sessions.find((thread) => thread.sessionId === sessionId);
  const targetHarness = selectedSession?.harness ?? harness;
  const update = (key: keyof Editor, value: string) => {
    setEdit((previous) => previous && { ...previous, [key]: value });
  };
  return (
    <section className="grid gap-5" aria-label="MCP management">
      <p className="text-sm text-muted">
        Manage servers on this node, including the servers configured in your harnesses.
      </p>
      <div className="flex flex-wrap items-end gap-3">
        <div className="grid min-w-48 flex-1 gap-2">
          <label className="text-sm" htmlFor={`${fieldPrefix}-harness`}>
            Harness
          </label>
          <Select
            aria-label="MCP harness"
            value={harness}
            onChange={(key) => {
              if (typeof key === "string") {
                ((value) => {
                  setHarness(value);
                  setSessionId("");
                })(key);
              }
            }}
            placeholder="Select…"
          >
            <Button variant="secondary" className="w-full" id={`${fieldPrefix}-harness`}>
              <span className="min-w-0 flex-1 truncate text-left">
                <Select.Value />
              </span>
              <ChevronDown size={16} className="shrink-0" aria-hidden="true" />
            </Button>
            <Select.Popover>
              <ListBox aria-label="MCP harness">
                {[
                  { id: "", label: "All harnesses" },
                  { id: "codex", label: "Codex" },
                  { id: "claude", label: "Claude Code" },
                  { id: "opencode", label: "OpenCode" },
                  { id: "pi", label: "Pi" },
                ].map((option) => (
                  <ListBox.Item
                    key={option.id}
                    id={option.id}
                    textValue={option.label}
                    isDisabled={"disabled" in option && option.disabled === true}
                  >
                    {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                    {option.label}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
        </div>
        <div className="grid min-w-48 flex-1 gap-2">
          <label className="text-sm" htmlFor={`${fieldPrefix}-session`}>
            Native session
          </label>
          <Select
            aria-label="MCP native session"
            value={sessionId}
            onChange={(key) => {
              if (typeof key === "string") {
                setSessionId(key);
              }
            }}
            placeholder="Select…"
          >
            <Button variant="secondary" className="w-full" id={`${fieldPrefix}-session`}>
              <span className="min-w-0 flex-1 truncate text-left">
                <Select.Value />
              </span>
              <ChevronDown size={16} className="shrink-0" aria-hidden="true" />
            </Button>
            <Select.Popover>
              <ListBox aria-label="MCP native session">
                {[
                  { id: "", label: "Configuration only" },
                  ...sessions
                    .filter((thread) => !harness || thread.harness === harness)
                    .map((thread) => ({
                      id: thread.sessionId ?? "",
                      label: `${thread.title} · ${thread.harness}`,
                    })),
                ].map((option) => (
                  <ListBox.Item
                    key={option.id}
                    id={option.id}
                    textValue={option.label}
                    isDisabled={"disabled" in option && option.disabled === true}
                  >
                    {"icon" in option && isValidElement(option.icon) ? option.icon : null}
                    {option.label}
                    <ListBox.ItemIndicator />
                  </ListBox.Item>
                ))}
              </ListBox>
            </Select.Popover>
          </Select>
        </div>
        <Button
          variant="outline"
          disabled={!!busy}
          onClick={() => {
            run("load", reload);
          }}
        >
          <RefreshCw size={14} />
          Refresh
        </Button>
        <Button
          disabled={!!busy}
          onClick={() => {
            setEdit(editor());
            setRemoving(undefined);
            action.reset();
          }}
        >
          <Plus size={14} />
          Add MCP
        </Button>
      </div>
      {!sessionId && (
        <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-project-directory`}>
          Project directory
          <Input
            id={`${fieldPrefix}-mcp-project-directory`}
            aria-label="MCP project directory"
            value={directoryDraft}
            onChange={(event) => {
              setDirectoryDraft(event.target.value);
            }}
            onBlur={() => {
              setCwd(directoryDraft.trim());
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                setCwd(directoryDraft.trim());
              }
            }}
            placeholder="Optional directory on this node"
          />
        </label>
      )}
      {error && (
        <p className="settings-error text-sm" role="alert">
          {error}
        </p>
      )}
      {hasText(notice) && <output className="text-sm">{notice}</output>}
      {hasText(authorization) && (
        <a
          className="text-sm underline underline-offset-4"
          href={authorization}
          target="_blank"
          rel="noreferrer"
        >
          Open MCP sign-in
        </a>
      )}
      {shown?.restartSessions.map((id) => (
        <div key={id} className="flex flex-wrap items-center gap-3 text-sm">
          <span>
            Native session requires reopening:{" "}
            {sessions.find((thread) => thread.sessionId === id)?.title ?? id}
          </span>
          <Button
            variant="outline"
            disabled={!!busy}
            onClick={() => {
              run(id, async () => {
                await store.rpc("mcp/reopen", { sessionId: id });
                const thread = sessions.find((item) => item.sessionId === id);
                if (thread) {
                  await store.restoreThread(thread.id);
                }
                await reload();
                return { notice: "Native session reopened. Its message queue remains paused." };
              });
            }}
          >
            Reopen session
          </Button>
        </div>
      ))}
      {edit !== undefined && (
        <McpServerEditor
          value={edit}
          busy={busy}
          targetHarness={targetHarness}
          selectedSession={selectedSession}
          cwd={cwd}
          update={update}
          onSave={() => {
            run(
              "save",
              async () =>
                await mutate(edit.entry === undefined ? "install" : "update", edit.entry, edit),
            );
          }}
          onCancel={() => {
            setEdit(undefined);
          }}
        />
      )}
      <Input
        aria-label="Search MCP servers"
        placeholder="Search installed MCP servers"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
        }}
      />
      <div className="divide-y divide-border">
        {entries.map((entry) => (
          <McpServerRow
            key={entry.id}
            entry={entry}
            busy={busy}
            confirming={removing?.id === entry.id}
            onToggle={(enabled) => {
              run(entry.id, async () => await mutate(enabled ? "enable" : "disable", entry));
            }}
            onEdit={() => {
              setEdit(editor(entry));
              setRemoving(undefined);
            }}
            onAuthenticate={() => {
              run(entry.id, async () => await mutate("authenticate", entry));
            }}
            onRemove={() => {
              setRemoving(entry);
              setEdit(undefined);
            }}
            onConfirm={() => {
              run(entry.id, async () => await mutate("uninstall", entry));
            }}
            onKeep={() => {
              setRemoving(undefined);
            }}
          />
        ))}
      </div>
      {busy === "load" && !shown && <output className="text-sm">Loading MCP configuration…</output>}
      {shown && !entries.length && (
        <p className="text-sm text-muted">
          {search
            ? "No matching MCP servers."
            : "No MCP servers configured. Add a server or select a project to discover its native configuration."}
        </p>
      )}
      {shown?.limitations.map((message) => (
        <p key={message} className="text-sm text-muted">
          {message}
        </p>
      ))}
    </section>
  );
}
