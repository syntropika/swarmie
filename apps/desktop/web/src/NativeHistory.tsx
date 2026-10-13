import { Disclosure } from "@/components/ui/disclosure";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { HarnessSelect } from "./HarnessSelect";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import type { WorkspaceStore } from "./workspace";
type HistorySession = {
  native: { provider: string; id: string };
  cwd: string;
  title: string;
  path: string;
  updatedAtMs: number;
};
type HistoryNode = {
  id: string;
  parentId?: string;
  role: string;
  text: string;
  active: boolean;
  metadata: { nativeAnchor?: boolean };
};
export function NativeHistory({ store }: { store: WorkspaceStore }) {
  const [harness, setHarness] = useState("codex");
  const [configHome, setConfigHome] = useState("");
  const [allProjects, setAllProjects] = useState(false);
  const [sessions, setSessions] = useState<HistorySession[]>([]);
  const [tree, setTree] = useState<HistoryNode[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<HistorySession>();
  const installed = store.getSnapshot().harnesses.filter((item) => item.available && ["codex", "claude", "pi"].includes(item.harness));
  const selectedHarness = installed.find((item) => item.harness === harness)?.harness ?? installed[0]?.harness ?? "";
  const cwd = allProjects ? undefined : store.active()?.worktreeId || undefined;
  const query = {
    harness: selectedHarness,
    cwd,
    configHome: configHome || undefined,
    limit: 100,
  };
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="native-history">
      <label>
        Harness
        <HarnessSelect label="CLI history harness" value={selectedHarness} harnesses={store.getSnapshot().harnesses} supported={["codex", "claude", "pi"]}
          onChange={(value) => { setHarness(value); setSessions([]); setSelected(undefined); setTree([]); }} />
      </label>
      <label>
        Configuration directory
        <Input
          value={configHome}
          onChange={(e) => setConfigHome(e.target.value)}
          placeholder="Use the harness default"
        />
      </label>
      <Switch label="Include other projects" checked={allProjects} onChange={setAllProjects} />
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy || !selectedHarness}
        onClick={() =>
          void run(async () => {
            setSessions(
              await store.rpc<HistorySession[]>("runtime/history", query),
            );
            setSelected(undefined);
            setTree([]);
          })
        }
      >
        {busy ? "Reading…" : "Find CLI conversations"}
      </Button>
      {error && (
        <p role="alert" className="inline-error">
          {error}
        </p>
      )}
      <div className="history-results">
        {sessions.map((session) => (
          <Button
            key={session.native.id}
            variant="ghost"
            className={`command-result ${selected?.native.id === session.native.id ? "selected" : ""}`}
            title={session.cwd}
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const result = await store.rpc<{ tree: HistoryNode[] }>(
                  "runtime/history/read",
                  { ...query, nativeId: session.native.id },
                );
                setSelected(session);
                setTree(result.tree);
              })
            }
          >
            {session.title || session.native.id}
            <span className="muted">
              {new Date(session.updatedAtMs).toLocaleDateString()}
            </span>
          </Button>
        ))}
      </div>
      {selected && (
        <>
          <p className="path-label">{selected.cwd}</p>
          <Button
            variant="ghost"
            className="primary-button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await store.importSession({ ...query, nativeId: selected.native.id });
              })
            }
          >
            Import conversation
          </Button>
          <Disclosure>
            <Disclosure.Summary>Native message tree · {tree.length} entries</Disclosure.Summary>
            <div className="native-tree">
              {tree.map((node) => (
                <Disclosure
                  key={node.id}
                  className={node.active ? "active-node" : ""}
                >
                  <Disclosure.Summary>
                    {node.role} · {node.id.slice(0, 10)}
                    {node.active ? " · active branch" : ""}
                  </Disclosure.Summary>
                  {node.parentId && (
                    <p className="muted">Parent: {node.parentId}</p>
                  )}
                  <pre>{node.text}</pre>
                </Disclosure>
              ))}
            </div>
          </Disclosure>
          <p className="panel-note">
            Import reads local history. Sending a message resumes the native
            session when supported.
          </p>
        </>
      )}
    </div>
  );
}
