import { useSource } from "@swarmie/client-ui";
import type { McpServer } from "@swarmie/plugin-mcp-browser";
import { Button, ChromeButton, Switch } from "@swarmie/ui-web";
import { ChevronRight, RotateCw, Settings } from "lucide-react";

import { useClient } from "../client/context";
import { IconButton, iconStyle } from "../controls";

function failed() {
  return undefined;
}
function connectionLabel(server: McpServer) {
  const { observation, definition } = server;
  if (observation.status === "connected") {
    return `Connected · ${observation.tools.length} tools`;
  }
  if (observation.status === "connecting") {
    return "Connecting";
  }
  if (observation.status === "unavailable" || observation.status === "closed") {
    return "Unavailable";
  }
  return definition.enabled ? "Not connected" : "Disabled";
}
function ServerRow({ server, open }: { server: McpServer; open: (id: string) => void }) {
  const { mcp } = useClient();
  const state = useSource(mcp.state);
  return (
    <div className="flex items-center gap-3 py-4">
      <button
        type="button"
        className="min-w-0 flex-1 text-left"
        onClick={() => {
          open(server.definition.id);
        }}
      >
        <span className="block truncate text-sm font-medium">{server.definition.name}</span>
        <span className="mt-1 block text-xs text-muted">{connectionLabel(server)}</span>
      </button>
      <IconButton
        icon={Settings}
        label={`Configure ${server.definition.name}`}
        onPress={() => {
          open(server.definition.id);
        }}
        disabled={state.busy !== null}
      />
      <Switch
        aria-label={`Enable ${server.definition.name}`}
        isSelected={server.definition.enabled}
        isDisabled={state.busy !== null || !state.available}
        onChange={(enabled) => {
          mcp.setEnabled(server.definition, enabled).catch(failed);
        }}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
        </Switch.Content>
      </Switch>
      {server.definition.enabled && server.observation.status !== "connected" ? (
        <IconButton
          icon={RotateCw}
          label={`Connect ${server.definition.name}`}
          onPress={() => {
            mcp.setEnabled(server.definition, true).catch(failed);
          }}
          disabled={state.busy !== null}
        />
      ) : null}
    </div>
  );
}
function ServerRows({
  query,
  open,
  add,
}: {
  query: string;
  open: (id: string) => void;
  add: () => void;
}) {
  const { mcp } = useClient();
  const state = useSource(mcp.state);
  const search = query.trim().toLowerCase();
  const list = state.servers.filter((entry) =>
    entry.definition.name.toLowerCase().includes(search),
  );
  if (state.loading) {
    return <output className="block py-10 text-sm text-muted">Loading MCP servers…</output>;
  }
  if (list.length > 0) {
    return (
      <div className="mt-4 divide-y divide-border rounded-xl border border-border bg-chrome-surface px-4">
        {list.map((server) => (
          <ServerRow key={server.definition.id} server={server} open={open} />
        ))}
      </div>
    );
  }
  return (
    <div className="mt-4 rounded-xl border border-border px-5 py-10 text-center">
      <h3 className="text-sm font-medium">
        {search === "" ? "No MCP servers yet" : "No matching MCP servers"}
      </h3>
      <p className="mt-2 text-sm leading-6 text-muted">
        {search === ""
          ? "Add a remote URL or an installed local command to connect your tools."
          : "Try another server name."}
      </p>
      {search === "" ? (
        <Button size="sm" variant="secondary" className="mt-5" onPress={add}>
          Add MCP server
        </Button>
      ) : null}
    </div>
  );
}
export function McpList({
  query,
  open,
  add,
}: {
  query: string;
  open: (id: string) => void;
  add: () => void;
}) {
  const { mcp, connectionState, navigate } = useClient();
  const state = useSource(mcp.state);
  const nodes = useSource(connectionState);
  const node = nodes.nodes.find((entry) => entry.id === nodes.selected);
  return (
    <div className="mt-10">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-medium">Servers</h2>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted">{node?.label}</span>
          <IconButton
            icon={RotateCw}
            label="Refresh MCP servers"
            onPress={() => {
              mcp.refresh().catch(failed);
            }}
            disabled={state.loading || state.busy !== null}
          />
        </div>
      </div>
      <ServerRows query={query} open={open} add={add} />
      {!state.available && !state.loading ? (
        <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
          <p className="text-sm text-muted">
            Connect an execution node with MCP support to manage its servers.
          </p>
          <ChromeButton
            size="action"
            onPress={() => {
              navigate({ settings: "connections" });
            }}
          >
            Node connections
            <ChevronRight className={iconStyle} />
          </ChromeButton>
        </div>
      ) : null}
      {state.error === null ? null : (
        <p role="alert" className="mt-5 text-sm text-danger">
          {state.error}
        </p>
      )}
    </div>
  );
}
