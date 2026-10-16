import type { InputNativeHistoryQuery, NativeHistorySession } from "@swarmie/contracts";
import { Button, Disclosure, Input, Switch } from "@swarmie/ui-web";
import { useIsFetching, useIsMutating, useMutation, useQuery } from "@tanstack/react-query";
import { useId, useState, useSyncExternalStore } from "react";

import { startAction } from "~/src/action-boundary";
import { HarnessSelect } from "~/src/harness-select";
import { nodeKey } from "~/src/node-scope";
import { nonEmptyText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

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
        <p className="text-sm text-muted">
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
        isSelected={allProjects}
        onChange={setAllProjects}
        isDisabled={busy || !state.connected}
        aria-label="Include other projects"
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          Include other projects
        </Switch.Content>
      </Switch>
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
        variant="tertiary"
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
            variant={selected?.native.id === session.native.id ? "secondary" : "tertiary"}
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
              <span className="text-xs text-muted">
                {new Date(session.updatedAtMs).toLocaleDateString()}
              </span>
            </span>
          </Button>
        ))}
      </div>
      {selected !== undefined && (
        <>
          <p className="font-mono text-xs break-all text-muted">{selected.cwd}</p>
          <Button
            variant="tertiary"
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
            <Disclosure.Heading>
              <Button slot="trigger" variant="tertiary">
                <span>Native message tree · {tree.data?.tree.length ?? 0} entries</span>
                <Disclosure.Indicator />
              </Button>
            </Disclosure.Heading>
            <Disclosure.Content>
              <Disclosure.Body>
                <div className="grid gap-2">
                  {tree.data?.tree.map((node) => (
                    <Disclosure key={node.id}>
                      <Disclosure.Heading>
                        <Button slot="trigger" variant="tertiary">
                          <span>
                            {node.role} · {node.id.slice(0, 10)}
                            {node.active ? " · active branch" : ""}
                          </span>
                          <Disclosure.Indicator />
                        </Button>
                      </Disclosure.Heading>
                      <Disclosure.Content>
                        <Disclosure.Body>
                          {node.parentId !== null && node.parentId.length > 0 && (
                            <p className="text-muted">Parent: {node.parentId}</p>
                          )}
                          <pre className="overflow-x-auto">{node.text}</pre>
                        </Disclosure.Body>
                      </Disclosure.Content>
                    </Disclosure>
                  ))}
                </div>
              </Disclosure.Body>
            </Disclosure.Content>
          </Disclosure>
          <p className="text-sm text-muted">
            Import reads local history. Sending a message resumes the native session when supported.
          </p>
        </>
      )}
    </>
  );
}
