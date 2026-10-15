import type { McpEntry, InputMcpDefinition as McpDefinition } from "@swarmie/contracts";
import { useId } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

import type { Thread } from "./protocol";
import { hasText } from "./value-presence";

export type Editor = {
  entry?: McpEntry;
  origin: string;
  name: string;
  scope: string;
  transport: string;
  command: string;
  args: string;
  url: string;
  envVars: string;
  bearer: string;
  headers: string;
};
const blank: Editor = {
  origin: "swarmie",
  name: "",
  scope: "user",
  transport: "stdio",
  command: "",
  args: "",
  url: "",
  envVars: "",
  bearer: "",
  headers: "",
};
export function editor(entry?: McpEntry): Editor {
  if (entry === undefined) {
    return { ...blank };
  }
  return {
    entry,
    origin: entry.origin,
    name: entry.name,
    scope: entry.scope,
    transport: entry.definition.transport,
    command: entry.definition.command ?? "",
    args: (entry.definition.args ?? []).join("\n"),
    url: entry.definition.url ?? "",
    envVars: (entry.definition.envVars ?? []).join("\n"),
    bearer: entry.definition.bearerTokenEnvVar ?? "",
    headers: Object.entries(entry.definition.headerEnvVars ?? {})
      .map(([key, value]) => `${key}=${value}`)
      .join("\n"),
  };
}
const lines = (text: string) =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
export function definition(value: Editor): McpDefinition {
  const headers: Record<string, string> = {};
  for (const line of lines(value.headers)) {
    const split = line.indexOf("=");
    if (split < 1 || line.slice(split + 1).trim().length === 0) {
      throw new Error("Enter one header and environment variable per line: Header=VARIABLE.");
    }
    headers[line.slice(0, split).trim()] = line.slice(split + 1).trim();
  }
  return value.transport === "stdio"
    ? {
        transport: value.transport,
        command: value.command.trim(),
        args: lines(value.args),
        envVars: lines(value.envVars),
      }
    : {
        transport: value.transport,
        url: value.url.trim(),
        bearerTokenEnvVar: hasText(value.bearer.trim()) ? value.bearer.trim() : null,
        headerEnvVars: headers,
      };
}
export function McpServerEditor({
  value: edit,
  busy,
  targetHarness,
  selectedSession,
  cwd,
  update,
  onSave,
  onCancel,
}: {
  value: Editor;
  busy: string;
  targetHarness: string;
  selectedSession: Thread | undefined;
  cwd: string;
  update: (key: keyof Editor, value: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const fieldPrefix = useId();
  let submitLabel = edit.entry === undefined ? "Install MCP" : "Save changes";
  if (busy === "save") {
    submitLabel = "Saving…";
  }
  return (
    <form
      className="grid gap-4 border-y border-border py-5"
      aria-label="MCP server editor"
      onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}
    >
      <h2 className="text-base font-medium">
        {edit.entry ? `Edit ${edit.name}` : "Add an MCP server"}
      </h2>
      {!edit.entry && (
        <div className="@container grid gap-3">
          <Select
            label="MCP configuration owner"
            value={edit.origin}
            onChange={(value) => {
              update("origin", value);
            }}
            options={[
              { id: "swarmie", label: "Swarmie · shared configuration" },
              {
                id: "native",
                label: "Selected harness · native configuration",
                disabled:
                  !hasText(targetHarness) ||
                  !["codex", "claude", "opencode", "pi"].includes(targetHarness) ||
                  selectedSession?.capabilities?.metadata?.mcpConfigurationScope === "external",
              },
            ]}
          />
          <Select
            label="MCP configuration scope"
            value={edit.scope}
            onChange={(value) => {
              update("scope", value);
            }}
            options={[
              { id: "user", label: "User" },
              {
                id: "project",
                label: "Project",
                disabled: !(hasText(selectedSession?.worktreeId) || cwd.trim().length > 0),
              },
            ]}
          />
        </div>
      )}
      {!edit.entry && (
        <p className="text-muted text-sm">
          {targetHarness
            ? `Applies to ${targetHarness} sessions on this node.`
            : "Applies to all compatible harnesses on this node."}
        </p>
      )}
      <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-server-name`}>
        Name
        <Input
          id={`${fieldPrefix}-mcp-server-name`}
          aria-label="MCP server name"
          required
          maxLength={128}
          value={edit.name}
          readOnly={!!edit.entry}
          onChange={(event) => {
            update("name", event.target.value);
          }}
        />
      </label>
      <Select
        label="MCP transport"
        value={edit.transport}
        disabled={!!edit.entry}
        onChange={(value) => {
          update("transport", value);
        }}
        options={[
          { id: "stdio", label: "STDIO · local process" },
          { id: "http", label: "Streamable HTTP · server URL" },
          ...(edit.transport === "sse" ? [{ id: "sse", label: "SSE" }] : []),
        ]}
      />
      {edit.transport === "stdio" ? (
        <>
          <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-command`}>
            Command
            <Input
              id={`${fieldPrefix}-mcp-command`}
              aria-label="MCP command"
              required
              value={edit.command}
              onChange={(event) => {
                update("command", event.target.value);
              }}
              placeholder="Executable available on this node"
            />
          </label>
          <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-arguments`}>
            Arguments
            <Textarea
              id={`${fieldPrefix}-mcp-arguments`}
              aria-label="MCP arguments"
              readOnly={edit.entry?.credentialFields.includes("args")}
              value={edit.args}
              onChange={(event) => {
                update("args", event.target.value);
              }}
              placeholder="One argument per line"
            />
          </label>
          <label
            className="grid gap-2 text-sm"
            htmlFor={`${fieldPrefix}-mcp-environment-variables`}
          >
            Environment variables
            <Textarea
              id={`${fieldPrefix}-mcp-environment-variables`}
              aria-label="MCP environment variables"
              value={edit.envVars}
              onChange={(event) => {
                update("envVars", event.target.value);
              }}
              placeholder="One variable name per line; values come from this node"
            />
          </label>
        </>
      ) : (
        <>
          <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-url`}>
            Server URL
            <Input
              id={`${fieldPrefix}-mcp-url`}
              aria-label="MCP URL"
              type="url"
              required
              value={edit.url}
              onChange={(event) => {
                update("url", event.target.value);
              }}
              placeholder="https://example.com/mcp"
            />
          </label>
          <label
            className="grid gap-2 text-sm"
            htmlFor={`${fieldPrefix}-mcp-bearer-token-variable`}
          >
            Bearer token environment variable
            <Input
              id={`${fieldPrefix}-mcp-bearer-token-variable`}
              aria-label="MCP bearer token variable"
              value={edit.bearer}
              onChange={(event) => {
                update("bearer", event.target.value);
              }}
              placeholder="Variable name, not the token"
            />
          </label>
          <label className="grid gap-2 text-sm" htmlFor={`${fieldPrefix}-mcp-header-variables`}>
            Header environment variables
            <Textarea
              id={`${fieldPrefix}-mcp-header-variables`}
              aria-label="MCP header variables"
              value={edit.headers}
              onChange={(event) => {
                update("headers", event.target.value);
              }}
              placeholder="Header=VARIABLE, one per line"
            />
          </label>
        </>
      )}
      {(edit.entry?.credentialFields.length ?? 0) > 0 && (
        <p className="text-muted text-sm">Saved native credentials are hidden and preserved.</p>
      )}
      <p className="text-muted text-sm">
        Adding a local MCP registers its command. The executable must already be available on this
        node.
      </p>
      <div className="flex gap-2">
        <Button type="submit" disabled={!!busy}>
          {submitLabel}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!!busy}
          onClick={() => {
            onCancel();
          }}
        >
          Cancel
        </Button>
      </div>
    </form>
  );
}
