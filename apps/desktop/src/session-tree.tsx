import { Button, Disclosure } from "@swarmie/ui-web";
import { useState } from "react";

import { startAction, errorMessage, errorReporter } from "~/src/action-boundary";
import { isRecord } from "~/src/checked-json";
import type { Thread } from "~/src/protocol";
import { hasText } from "~/src/value-presence";
import type { WorkspaceStore } from "~/src/workspace";

async function readTree(store: WorkspaceStore, sessionId: string) {
  return await store.rpc("session/tree", { sessionId });
}

export function SessionTree({ thread, store }: { thread: Thread; store: WorkspaceStore }) {
  const [nodes, setNodes] = useState<Awaited<ReturnType<typeof readTree>>>([]);
  const [busy, setBusy] = useState(false);
  const [issue, setIssue] = useState("");
  const metadata = thread.capabilities?.metadata;
  const before = metadata?.forkBeforeUser === true;
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setIssue("");
    try {
      await action();
    } catch (error) {
      setIssue(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Disclosure
      onExpandedChange={(open) => {
        if (open && !nodes.length && hasText(thread.sessionId)) {
          startAction(
            run(async () => {
              setNodes(
                await store.rpc("session/tree", {
                  sessionId: thread.sessionId,
                }),
              );
            }),
            errorReporter(setIssue),
          );
        }
      }}
    >
      <Disclosure.Heading>
        <Button slot="trigger" variant="tertiary">
          <span>Native conversation tree</span>
          <Disclosure.Indicator />
        </Button>
      </Disclosure.Heading>
      <Disclosure.Content>
        <Disclosure.Body>
          {issue && (
            <p className="inline-error" role="alert">
              {issue}
            </p>
          )}
          <Button
            variant="tertiary"
            size="sm"
            disabled={busy}
            onClick={() => {
              startAction(
                run(async () => {
                  setNodes(await store.rpc("session/tree", { sessionId: thread.sessionId }));
                }),
                errorReporter(setIssue),
              );
            }}
          >
            Refresh tree
          </Button>
          {nodes.map((node) => (
            <Disclosure key={node.id}>
              <Disclosure.Heading>
                <Button slot="trigger" variant="tertiary">
                  <span>
                    {node.role} · {node.id.slice(0, 10)}
                    {node.active ? " · active" : ""}
                  </span>
                  <Disclosure.Indicator />
                </Button>
              </Disclosure.Heading>
              <Disclosure.Content>
                <Disclosure.Body>
                  {hasText(node.parentId) && <p className="muted">Parent: {node.parentId}</p>}
                  <pre className="action-output">{node.text}</pre>
                  {before &&
                    node.role === "user" &&
                    (!isRecord(node.metadata) || node.metadata.nativeAnchor !== false) &&
                    ["idle", "closed"].includes(thread.state) && (
                      <Button
                        variant="tertiary"
                        size="sm"
                        disabled={busy}
                        onClick={() => {
                          startAction(
                            run(async () => {
                              await store.editInBranch(thread.id, { id: node.id, text: node.text });
                            }),
                            errorReporter(setIssue),
                          );
                        }}
                      >
                        Edit in a new branch
                      </Button>
                    )}
                </Disclosure.Body>
              </Disclosure.Content>
            </Disclosure>
          ))}
          {!busy && !issue && !nodes.length && (
            <p className="panel-note">No native message entries are available.</p>
          )}
        </Disclosure.Body>
      </Disclosure.Content>
    </Disclosure>
  );
}
