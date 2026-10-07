import { Disclosure } from "@/components/ui/disclosure";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import type { Thread, PromptPart } from "./protocol";
import type { WorkspaceStore } from "./workspace";
type Node = {
  id: string;
  parentId?: string;
  role: string;
  text: string;
  active: boolean;
  metadata?: { nativeAnchor?: boolean; prompt?: { content: PromptPart[] } };
};
export function SessionTree({
  thread,
  store,
}: {
  thread: Thread;
  store: WorkspaceStore;
}) {
  const [nodes, setNodes] = useState<Node[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const metadata = thread.capabilities?.metadata as
    Record<string, unknown> | undefined;
  const before = metadata?.forkBeforeUser === true;
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
    <Disclosure
      onOpenChange={(open) => {
        if (open && !nodes.length && thread.sessionId)
          void run(async () =>
            setNodes(
              await store.rpc<Node[]>("session/tree", {
                sessionId: thread.sessionId,
              }),
            ),
          );
      }}
    >
      <Disclosure.Summary>Native conversation tree</Disclosure.Summary>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <Button
        variant="ghost"
        className="small-button"
        disabled={busy}
        onClick={() =>
          void run(async () =>
            setNodes(
              await store.rpc("session/tree", { sessionId: thread.sessionId }),
            ),
          )
        }
      >
        Refresh tree
      </Button>
      {nodes.map((node) => (
        <Disclosure className="native-tree-node" key={node.id}>
          <Disclosure.Summary>
            {node.role} · {node.id.slice(0, 10)}
            {node.active ? " · active" : ""}
          </Disclosure.Summary>
          {node.parentId && <p className="muted">Parent: {node.parentId}</p>}
          <pre className="action-output">{node.text}</pre>
          {before &&
            node.role === "user" &&
            node.metadata?.nativeAnchor !== false &&
            ["idle", "closed"].includes(thread.state) && (
              <Button
                variant="ghost"
                className="small-button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await store.editInBranch(thread.id, node);
                  })
                }
              >
                Edit in a new branch
              </Button>
            )}
        </Disclosure>
      ))}
      {!busy && !error && !nodes.length && (
        <p className="panel-note">No native message entries are available.</p>
      )}
    </Disclosure>
  );
}
