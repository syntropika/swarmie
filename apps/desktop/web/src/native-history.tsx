import type { InputNativeHistoryQuery, NativeHistorySession } from "@swarmie/contracts";
import { useIsFetching, useIsMutating, useMutation, useQuery } from "@tanstack/react-query";
import { useId, useState, useSyncExternalStore } from "react";

import { startAction } from "~/web/src/action-boundary";
import { Button } from "~/web/src/components/ui/button";
import { Disclosure } from "~/web/src/components/ui/disclosure";
import { Input } from "~/web/src/components/ui/input";
import { Switch } from "~/web/src/components/ui/switch";
import { HarnessSelect } from "~/web/src/harness-select";
import { nodeKey } from "~/web/src/node-scope";
import { nonEmptyText } from "~/web/src/value-presence";
import type { WorkspaceStore } from "~/web/src/workspace";

const supported = ["codex", "claude", "cursor", "pi"];
export function NativeHistory({ store }: { store: WorkspaceStore }) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const [harness, setHarness] = useState("codex");
  const [configHome, setConfigHome] = useState("");
  const [allProjects, setAllProjects] = useState(false);
  const fieldId = useId();
  const installed = state.harnesses.filter(
    (item) =>
      item.available &&
      !state.disabledHarnesses.includes(item.harness) &&
      supported.includes(item.harness),
  );
  const selectedHarness =
    installed.find((item) => item.harness === harness)?.harness ?? installed.at(0)?.harness ?? "";
  const scope = state.node ? nodeKey(state.node) : "";
  const query: InputNativeHistoryQuery = {
    harness: selectedHarness,
    cwd: allProjects ? undefined : nonEmptyText(store.active()?.worktreeId),
    configHome: nonEmptyText(configHome),
    limit: 100,
  };
  const busy =
    useIsFetching({ queryKey: ["native-history", scope, query] }) +
      useIsFetching({ queryKey: ["native-history-tree", scope, query] }) +
      useIsMutating({ mutationKey: ["native-history-import", scope, query] }) >
    0;
  const selection = JSON.stringify([scope, state.connected, query]);
  return (
    <div className="grid gap-3">
      <fieldset>
        <legend className="mb-1 text-sm">Agent tool</legend>
        <HarnessSelect
          label="Conversation history agent"
          value={selectedHarness}
          harnesses={state.harnesses}
          disabledHarnesses={state.disabledHarnesses}
          supported={supported}
          disabled={busy || !state.connected}
          onChange={setHarness}
        />
      </fieldset>
      {selectedHarness === "cursor" && (
        <p className="text-muted text-sm">
          Cursor history reads local SDK conversations. Cursor CLI conversations use a separate
          store.
        </p>
      )}
      <label htmlFor={fieldId}>
        {selectedHarness === "cursor" ? "SDK state directory" : "Configuration directory"}
        <Input
          id={fieldId}
          value={configHome}
          onChange={(event) => {
            setConfigHome(event.target.value);
          }}
          placeholder={
            selectedHarness === "cursor"
              ? "Use Cursor's existing SDK stores"
              : "Use the agent default"
          }
          disabled={busy || !state.connected}
        />
      </label>
      <Switch
        label="Include other projects"
        checked={allProjects}
        onChange={setAllProjects}
        disabled={busy || !state.connected}
      />
      <HistoryResults
        key={selection}
        store={store}
        query={query}
        connected={state.connected}
        scope={scope}
      />
    </div>
  );
}
function HistoryResults({
  store,
  query,
  connected,
  scope,
}: {
  store: WorkspaceStore;
  query: InputNativeHistoryQuery;
  connected: boolean;
  scope: string;
}) {
  const [selected, setSelected] = useState<NativeHistorySession>();
  const [error, setError] = useState("");
  const sessions = useQuery({
    queryKey: ["native-history", scope, query],
    queryFn: async () => await store.rpc("runtime/history", query),
    enabled: false,
  });
  const tree = useQuery({
    queryKey: ["native-history-tree", scope, query, selected?.native.id],
    queryFn: async () => {
      if (selected === undefined) {
        throw new Error("Select a native conversation first");
      }
      return await store.rpc("runtime/history/read", { ...query, nativeId: selected.native.id });
    },
    enabled: connected && selected !== undefined,
  });
  const importing = useMutation({
    mutationKey: ["native-history-import", scope, query],
    mutationFn: async (nativeId: string) => {
      await store.importSession({ ...query, nativeId });
    },
  });
  const busy = sessions.isFetching || tree.isFetching || importing.isPending;
  const read = async () => {
    setError("");
    setSelected(undefined);
    const result = await sessions.refetch();
    if (result.isError) {
      setError("Native conversations could not be read. Try again.");
    }
  };
  const select = (session: NativeHistorySession) => {
    setError("");
    setSelected(session);
  };
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        disabled={busy || query.harness.length === 0 || !connected}
        onClick={() => {
          startAction(read(), () => {
            setError("Native conversations could not be read. Try again.");
          });
        }}
      >
        {sessions.isFetching ? "Reading…" : "Find native conversations"}
      </Button>
      {(error.length > 0 || sessions.isError || tree.isError || importing.isError) && (
        <p role="alert" className="inline-error">
          {error.length > 0
            ? error
            : "This native conversation could not be opened. Refresh its history and try again."}
        </p>
      )}
      <div className="grid gap-1">
        {sessions.data?.map((session) => (
          <Button
            key={session.native.id}
            variant={selected?.native.id === session.native.id ? "secondary" : "ghost"}
            className="w-full"
            title={session.cwd}
            disabled={busy || !connected}
            onClick={() => {
              select(session);
            }}
          >
            <span className="flex w-full min-w-0 items-center justify-between gap-2">
              <span className="truncate">
                {session.title.length > 0 ? session.title : session.native.id}
              </span>
              <span className="text-muted text-xs">
                {new Date(session.updatedAtMs).toLocaleDateString()}
              </span>
            </span>
          </Button>
        ))}
      </div>
      {selected !== undefined && (
        <>
          <p className="text-muted font-mono text-xs break-all">{selected.cwd}</p>
          <Button
            variant="ghost"
            disabled={busy || !connected}
            onClick={() => {
              startAction(importing.mutateAsync(selected.native.id), () => {
                setError("The native conversation could not be imported. Try again.");
              });
            }}
          >
            Import conversation
          </Button>
          <Disclosure>
            <Disclosure.Summary>
              Native message tree · {tree.data?.tree.length ?? 0} entries
            </Disclosure.Summary>
            <div className="grid gap-2">
              {tree.data?.tree.map((node) => (
                <Disclosure key={node.id}>
                  <Disclosure.Summary>
                    {node.role} · {node.id.slice(0, 10)}
                    {node.active ? " · active branch" : ""}
                  </Disclosure.Summary>
                  {node.parentId !== null && node.parentId.length > 0 && (
                    <p className="text-muted">Parent: {node.parentId}</p>
                  )}
                  <pre className="overflow-x-auto">{node.text}</pre>
                </Disclosure>
              ))}
            </div>
          </Disclosure>
          <p className="text-muted text-sm">
            Import reads local history. Sending a message resumes the native session when supported.
          </p>
        </>
      )}
    </>
  );
}
