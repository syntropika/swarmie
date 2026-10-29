import { useSource } from "@swarmie/client-ui";
import { ActionMenuPopover, Button, Dropdown, Label, SearchField, Tabs } from "@swarmie/ui-web";
import { ChevronDown, Globe, Terminal } from "lucide-react";
import { useState } from "react";

import { useClient } from "../client/context";
import { McpEditor } from "./mcp-editor";
import { McpList } from "./mcp-list";
import { PluginDetail } from "./plugin-detail";
import { PluginList } from "./plugin-list";

type View =
  | { kind: "list" }
  | { kind: "plugin"; id: string }
  | { kind: "mcp"; id: string }
  | { kind: "add"; transport: "http" | "local" };

export function PluginSettings() {
  const { plugins, mcp, connectionState } = useClient();
  const list = useSource(plugins);
  const state = useSource(mcp.state);
  const nodes = useSource(connectionState);
  const [filter, setFilter] = useState<"plugins" | "mcps">("plugins");
  const [query, setQuery] = useState("");
  const [view, setView] = useState<View>({ kind: "list" });
  const back = () => {
    setView({ kind: "list" });
  };
  const openPlugin = (id: string) => {
    setView({ kind: "plugin", id });
  };
  const openMcp = (id: string) => {
    setView({ kind: "mcp", id });
  };
  const selected =
    view.kind === "mcp"
      ? state.servers.find((entry) => entry.definition.id === view.id)
      : undefined;
  function select(value: string | number) {
    if (value === "plugins" || value === "mcps") {
      setFilter(value);
      back();
    }
  }
  function add(transport: "http" | "local") {
    setFilter("mcps");
    setView({ kind: "add", transport });
  }
  const node = nodes.nodes.find((entry) => entry.id === nodes.selected);
  const identity = `${node?.id ?? ""}:${node?.principal ?? ""}`;
  const searchLabel = filter === "mcps" ? "Search MCP servers" : "Search plugins";
  let pluginContent = <PluginList query={query} open={openPlugin} />;
  if (view.kind === "plugin") {
    pluginContent = (
      <PluginDetail
        id={view.id}
        back={back}
        openPlugin={(id) => {
          if (id === null) {
            back();
          } else {
            openPlugin(id);
          }
        }}
      />
    );
  }
  let mcpContent = (
    <McpList
      query={query}
      open={openMcp}
      add={() => {
        add("http");
      }}
    />
  );
  if (view.kind === "add") {
    mcpContent = (
      <McpEditor key={`${identity}:new:${view.transport}`} transport={view.transport} back={back} />
    );
  }
  if (selected) {
    mcpContent = (
      <McpEditor
        key={`${identity}:${selected.definition.id}`}
        server={selected}
        transport={selected.definition.endpoint.kind}
        back={back}
      />
    );
  }
  return (
    <div className="@container">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-medium">Plugins</h1>
          <p className="mt-2 text-sm text-muted">Manage plugins and MCP servers.</p>
        </div>
        <div className="flex items-center gap-2">
          <Dropdown>
            <Button aria-label="Add integration" size="sm">
              Add
              <ChevronDown />
            </Button>
            <ActionMenuPopover>
              <Dropdown.Menu
                aria-label="Add integration"
                onAction={(key) => {
                  if (key === "http" || key === "local") {
                    add(key);
                  }
                }}
              >
                <Dropdown.Item id="http" textValue="Remote MCP server">
                  <Globe />
                  <Label>Remote MCP server</Label>
                </Dropdown.Item>
                <Dropdown.Item id="local" textValue="Local MCP server">
                  <Terminal />
                  <Label>Local MCP server</Label>
                </Dropdown.Item>
              </Dropdown.Menu>
            </ActionMenuPopover>
          </Dropdown>
        </div>
      </header>
      <Tabs selectedKey={filter} onSelectionChange={select} className="mt-9 w-full">
        <div className="flex flex-col gap-4 @lg:flex-row @lg:items-center @lg:justify-between">
          <Tabs.ListContainer>
            <Tabs.List aria-label="Integration types">
              <Tabs.Tab id="plugins">
                Plugins <span className="ml-1 text-muted">{list.length}</span>
                <Tabs.Indicator />
              </Tabs.Tab>
              <Tabs.Tab id="mcps">
                MCPs <span className="ml-1 text-muted">{state.servers.length}</span>
                <Tabs.Indicator />
              </Tabs.Tab>
            </Tabs.List>
          </Tabs.ListContainer>
          <SearchField
            aria-label={searchLabel}
            value={query}
            onChange={setQuery}
            className="w-full @lg:w-56"
          >
            <SearchField.Group>
              <SearchField.SearchIcon />
              <SearchField.Input placeholder={searchLabel} />
              <SearchField.ClearButton />
            </SearchField.Group>
          </SearchField>
        </div>
        <Tabs.Panel id="plugins">
          <div className="mt-8">{pluginContent}</div>
        </Tabs.Panel>
        <Tabs.Panel id="mcps">{mcpContent}</Tabs.Panel>
      </Tabs>
    </div>
  );
}
