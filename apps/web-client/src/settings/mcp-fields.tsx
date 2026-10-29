import { Button, Input, Label, ListBox, Select, TextArea, TextField } from "@swarmie/ui-web";
import { Plus, Trash2 } from "lucide-react";

import type { ChangeDraft, McpDraft } from "./mcp-draft";
import { draftId } from "./mcp-draft";

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
      <div>
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
      <section className="min-w-0" aria-label="HTTP headers">
        <h3 className="text-sm font-medium">Headers</h3>
        <p className="mt-1 text-xs leading-5 text-muted">
          Add non-secret headers. Do not enter tokens, passwords or cookies.
        </p>
        <div className="@container/mcp-headers mt-3 grid gap-3">
          {draft.headers.map((header) => (
            <div
              key={header.id}
              className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 @min-[384px]/mcp-headers:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
            >
              <TextField
                className="col-start-1 row-start-1 min-w-0"
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
                className="col-start-1 row-start-2 min-w-0 @min-[384px]/mcp-headers:col-start-2 @min-[384px]/mcp-headers:row-start-1"
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
                variant="tertiary"
                className="col-start-2 row-span-2 row-start-1 @min-[384px]/mcp-headers:col-start-3 @min-[384px]/mcp-headers:row-span-1"
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
                headers: [...draft.headers, { id: draftId(), name: "", value: "" }],
              });
            }}
            isDisabled={busy || draft.headers.length >= 32}
          >
            <Plus />
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
      <div>
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
      <div>
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
      <div>
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
    <div>
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
