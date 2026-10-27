import { useSource } from "@swarmie/client-ui";
import { Button, ChromeButton, Input, Label, TextField } from "@swarmie/ui-web";
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
      <div className="mt-8 grid gap-4 rounded-xl border border-border bg-chrome-surface p-4">
        {connection.nodes.map((node) => {
          const session = connection.sessions[node.id];
          const connected = session?.phase === "connected";
          return (
            <div key={node.id} className="grid gap-2">
              <div className="flex items-center gap-3">
                <Monitor className="size-5 text-muted" />
                <div className="flex-1">
                  <h2 className="text-sm">{node.label}</h2>
                  <output className="mt-1 text-xs text-muted">
                    {session?.phase ?? "unconfigured"}
                  </output>
                </div>
                <ChromeButton
                  size="action"
                  selected={connection.selected === node.id}
                  aria-pressed={connection.selected === node.id}
                  onPress={() => {
                    selectNode(node.id);
                  }}
                >
                  {connection.selected === node.id ? "Default" : "Use as default"}
                </ChromeButton>
                {node.url === null ? null : (
                  <ChromeButton
                    size="action"
                    onPress={() => {
                      if (connected) {
                        disconnect(node.id);
                      } else {
                        connect(node.id).catch(failed);
                      }
                    }}
                  >
                    {connected ? "Disconnect" : "Connect"}
                  </ChromeButton>
                )}
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
