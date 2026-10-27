import { useSource } from "@swarmie/client-ui";
import type { McpServer } from "@swarmie/plugin-mcp-browser";
import { Button, ChromeButton, Input, Label, TextField } from "@swarmie/ui-web";
import { ArrowLeft, Trash2 } from "lucide-react";
import { useRef, useState } from "react";

import { useClient } from "../client/context";
import { iconStyle } from "../controls";
import { createDraft, draftDefinition } from "./mcp-draft";
import type { ChangeDraft } from "./mcp-draft";
import { HttpFields, LocalFields, ProtocolField } from "./mcp-fields";

export function McpEditor({
  server,
  transport,
  back,
}: {
  server?: McpServer;
  transport: "http" | "local";
  back: () => void;
}) {
  const { mcp, connectionState } = useClient();
  const state = useSource(mcp.state);
  const connections = useSource(connectionState);
  const [draft, setDraft] = useState(() => createDraft(transport, server?.definition));
  const original = useRef(server?.definition);
  const [formError, setFormError] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  const busy = state.busy !== null;
  const editing = draft.expectedRevision !== null;
  const change: ChangeDraft = (patch) => {
    setDraft((current) => ({ ...current, ...patch }));
  };
  function failed(reason: unknown) {
    setFormError(reason instanceof Error ? reason.message : String(reason));
  }
  async function save() {
    setFormError(null);
    try {
      await mcp.save(draftDefinition(draft), draft.expectedRevision);
      back();
    } catch (error: unknown) {
      failed(error);
    }
  }
  async function remove() {
    try {
      if (!original.current) {
        throw new Error("The server definition is unavailable.");
      }
      await mcp.remove(original.current);
      back();
    } catch (error: unknown) {
      failed(error);
    }
  }
  const node = connections.nodes.find((entry) => entry.id === connections.selected);
  return (
    <div className="mt-10">
      <ChromeButton size="action" onPress={back}>
        <ArrowLeft className={iconStyle} />
        Back
      </ChromeButton>
      <div className="mt-6 flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-xl font-medium">
          {editing ? `Update ${draft.initialName}` : "Add MCP server"}
        </h2>
        {editing ? (
          <Button
            size="sm"
            variant="danger-soft"
            onPress={() => {
              setRemoving(true);
            }}
            isDisabled={busy}
          >
            <Trash2 />
            Remove
          </Button>
        ) : null}
      </div>
      <p className="mt-3 text-sm leading-6 text-muted">
        {transport === "http"
          ? "Connect a remote MCP server by URL."
          : "Run an installed MCP server on the execution node."}{" "}
        Settings apply to {node?.label ?? "the selected node"}.
      </p>
      {editing ? (
        <p className="mt-2 text-xs text-muted">
          Saving stops new calls until you enable the updated server. To change its connection type,
          remove it first.
        </p>
      ) : null}
      <form
        className="mt-6"
        onSubmit={(event) => {
          event.preventDefault();
          save().catch(failed);
        }}
      >
        <fieldset
          disabled={busy}
          className="min-w-0 divide-y divide-border rounded-xl border border-border bg-chrome-surface"
        >
          <div className="p-4">
            <TextField
              value={draft.name}
              onChange={(name) => {
                change({ name });
              }}
              isRequired
            >
              <Label>Server name</Label>
              <Input fullWidth placeholder="e.g. Documents" maxLength={128} />
            </TextField>
          </div>
          {transport === "http" ? (
            <HttpFields draft={draft} change={change} busy={busy} />
          ) : (
            <LocalFields draft={draft} change={change} />
          )}
          <ProtocolField draft={draft} change={change} />
        </fieldset>
        {state.available ? null : (
          <p className="mt-4 text-sm text-muted">
            Connect an execution node with MCP support to save this server.
          </p>
        )}
        {formError === null ? null : (
          <p role="alert" className="mt-4 text-sm text-danger">
            {formError}
          </p>
        )}
        <div className="mt-5 flex justify-end">
          <Button
            type="submit"
            loading={busy}
            loadingLabel="Saving MCP server"
            isDisabled={!state.available || draft.name.trim() === ""}
          >
            Save
          </Button>
        </div>
      </form>
      {removing ? (
        <div role="alert" className="mt-5 rounded-xl border border-border p-4">
          <h3 className="text-sm font-medium">Remove {draft.initialName}?</h3>
          <p className="mt-2 text-sm text-muted">
            This removes its configuration and stops new calls. It does not uninstall the server
            program.
          </p>
          <div className="mt-4 flex justify-end gap-2">
            <Button
              size="sm"
              variant="secondary"
              onPress={() => {
                setRemoving(false);
              }}
            >
              Cancel
            </Button>
            <Button
              size="sm"
              variant="danger"
              onPress={() => {
                remove().catch(failed);
              }}
              loading={busy}
            >
              Remove server
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
