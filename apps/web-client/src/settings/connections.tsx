import { useSource } from "@swarmie/client-ui";
import { Button, Input, Label, TextField } from "@swarmie/ui-web";
import { Monitor } from "lucide-react";
import { useState } from "react";

import { useClient } from "../client/context";

export function ConnectionSettings() {
  const { connectionState, configureNode, connect, disconnect, selectNode } = useClient();
  const connection = useSource(connectionState);
  const [url, setUrl] = useState("");
  const [nodeId, setNodeId] = useState("");
  const [principal, setPrincipal] = useState("");
  const [connectionError, setConnectionError] = useState<string | null>(null);
  function failed(error: unknown) {
    setConnectionError(error instanceof Error ? error.message : String(error));
  }
  return (
    <>
      <h1 className="text-2xl font-medium">Connections</h1>
      <p className="mt-2 text-xs text-muted">
        Select a default node while keeping other nodes connected.
      </p>
      <div className="mt-8 grid gap-6">
        {connection.nodes.map((node) => {
          const session = connection.sessions[node.id];
          const connected = session?.phase === "connected";
          return (
            <div
              key={node.id}
              className="@container/connection grid gap-2 rounded-group bg-group p-4"
            >
              <div className="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-3 @min-[480px]/connection:grid-cols-[auto_minmax(0,1fr)_auto]">
                <Monitor className="size-5 shrink-0 text-muted" />
                <div className="min-w-0 wrap-anywhere">
                  <h2 className="text-sm">{node.label}</h2>
                  <output className="mt-1 text-xs text-muted">
                    {session?.phase ?? "unconfigured"}
                  </output>
                </div>
                <div className="col-span-2 flex flex-wrap items-center justify-end gap-2 @min-[480px]/connection:col-span-1">
                  <Button
                    size="sm"
                    variant="secondary"
                    aria-pressed={connection.selected === node.id}
                    onPress={() => {
                      selectNode(node.id);
                    }}
                  >
                    {connection.selected === node.id ? "Default" : "Use as default"}
                  </Button>
                  {node.url === null ? null : (
                    <Button
                      size="sm"
                      variant="secondary"
                      onPress={() => {
                        if (connected) {
                          disconnect(node.id);
                        } else {
                          connect(node.id).catch(failed);
                        }
                      }}
                    >
                      {connected ? "Disconnect" : "Connect"}
                    </Button>
                  )}
                </div>
              </div>
              {session?.failure ? (
                <p role="alert" className="text-xs text-danger">
                  {session.failure.message}
                </p>
              ) : null}
            </div>
          );
        })}
        <form
          onSubmit={(event) => {
            event.preventDefault();
            try {
              const target = new URL(url);
              if (target.protocol !== "ws:" && target.protocol !== "wss:") {
                throw new Error("Use a WebSocket node URL.");
              }
              if (nodeId.trim() === "" || principal.trim() === "") {
                throw new Error("Provide the expected node and user identities.");
              }
              setConnectionError(null);
              configureNode({
                id: nodeId.trim(),
                label: nodeId.trim(),
                url: target.href,
                principal: principal.trim(),
              }).catch(failed);
            } catch (error: unknown) {
              failed(error);
            }
          }}
        >
          <div className="grid gap-3">
            <TextField value={nodeId} onChange={setNodeId} isRequired validationBehavior="native">
              <Label>Node ID</Label>
              <Input fullWidth />
            </TextField>
            <TextField
              value={principal}
              onChange={setPrincipal}
              isRequired
              validationBehavior="native"
            >
              <Label>Node user</Label>
              <Input fullWidth />
            </TextField>
            <TextField
              value={url}
              onChange={setUrl}
              type="url"
              isRequired
              validationBehavior="native"
            >
              <Label>Node URL</Label>
              <Input fullWidth placeholder="ws://localhost:8080/rpc" />
            </TextField>
          </div>
          <div className="mt-3 flex justify-end">
            <Button type="submit" variant="secondary">
              Connect node
            </Button>
          </div>
        </form>
        {connectionError === null ? null : (
          <p role="alert" className="text-xs text-danger">
            {connectionError}
          </p>
        )}
      </div>
    </>
  );
}
