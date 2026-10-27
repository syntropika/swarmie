import { Button, Input, Label, ListBox, Select, TextArea, TextField } from "@swarmie/ui-web";
import { Plus, Trash2 } from "lucide-react";

import type { ChangeDraft, McpDraft } from "./mcp-draft";

export function HttpFields({
  draft,
  change,
  busy,
}: {
  draft: McpDraft;
  change: ChangeDraft;
  busy: boolean;
}) {
  return (
    <>
      <div className="p-4">
        <TextField
          value={draft.url}
          onChange={(url) => {
            change({ url });
          }}
          isRequired
          type="url"
        >
          <Label>URL</Label>
          <Input fullWidth placeholder="https://example.com/mcp" />
        </TextField>
      </div>
      <section className="p-4" aria-label="HTTP headers">
        <h3 className="text-sm font-medium">Headers</h3>
        <p className="mt-1 text-xs leading-5 text-muted">
          Add non-secret headers. Do not enter tokens, passwords or cookies.
        </p>
        <div className="mt-3 grid gap-3">
          {draft.headers.map((header) => (
            <div key={header.id} className="flex items-center gap-2">
              <TextField
                className="min-w-0 flex-1"
                aria-label="Header name"
                value={header.name}
                onChange={(name) => {
                  change({
                    headers: draft.headers.map((entry) =>
                      entry.id === header.id ? { ...entry, name } : entry,
                    ),
                  });
                }}
              >
                <Input fullWidth placeholder="Name" />
              </TextField>
              <TextField
                className="min-w-0 flex-1"
                aria-label="Header value"
                value={header.value}
                onChange={(value) => {
                  change({
                    headers: draft.headers.map((entry) =>
                      entry.id === header.id ? { ...entry, value } : entry,
                    ),
                  });
                }}
              >
                <Input fullWidth placeholder="Value" />
              </TextField>
              <Button
                aria-label="Remove header"
                isIconOnly
                size="sm"
                shape="panel"
                variant="tertiary"
                onPress={() => {
                  change({ headers: draft.headers.filter((entry) => entry.id !== header.id) });
                }}
                isDisabled={busy}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
          <Button
            variant="tertiary"
            size="sm"
            onPress={() => {
              change({
                headers: [...draft.headers, { id: crypto.randomUUID(), name: "", value: "" }],
              });
            }}
            isDisabled={busy || draft.headers.length >= 32}
          >
            <Plus className="size-3.5" />
            Add header
          </Button>
        </div>
      </section>
    </>
  );
}
export function LocalFields({ draft, change }: { draft: McpDraft; change: ChangeDraft }) {
  return (
    <>
      <div className="p-4">
        <TextField
          value={draft.program}
          onChange={(program) => {
            change({ program });
          }}
          isRequired
        >
          <Label>Command</Label>
          <Input fullWidth placeholder="e.g. npx" />
        </TextField>
      </div>
      <div className="p-4">
        <TextField
          value={draft.argumentsText}
          onChange={(argumentsText) => {
            change({ argumentsText });
          }}
        >
          <Label>Arguments (JSON array)</Label>
          <TextArea fullWidth rows={3} spellCheck={false} />
        </TextField>
      </div>
      <div className="p-4">
        <TextField
          value={draft.directory}
          onChange={(directory) => {
            change({ directory });
          }}
        >
          <Label>Working directory (optional)</Label>
          <Input fullWidth placeholder="Use the execution node's default directory" />
        </TextField>
      </div>
    </>
  );
}
export function ProtocolField({ draft, change }: { draft: McpDraft; change: ChangeDraft }) {
  return (
    <div className="p-4">
      <Select
        value={draft.protocol}
        onChange={(value) => {
          if (value === "2026-07-28" || value === "2025-11-25") {
            change({ protocol: value });
          }
        }}
      >
        <Label>Protocol version</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>
            {["2026-07-28", "2025-11-25"].map((version) => (
              <ListBox.Item key={version} id={version} textValue={version}>
                {version}
                <ListBox.ItemIndicator />
              </ListBox.Item>
            ))}
          </ListBox>
        </Select.Popover>
      </Select>
    </div>
  );
}
