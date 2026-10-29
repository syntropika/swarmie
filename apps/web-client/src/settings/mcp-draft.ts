import type { McpDefinition, McpProtocol, McpScope } from "@swarmie/plugin-mcp-browser";

import { randomId as draftId } from "../id";

export { randomId as draftId } from "../id";

export type Header = { id: string; name: string; value: string };
export type McpDraft = {
  id: string;
  name: string;
  initialName: string;
  scope: McpScope;
  expectedRevision: number | null;
  transport: "http" | "local";
  url: string;
  program: string;
  argumentsText: string;
  directory: string;
  protocol: McpProtocol;
  headers: Header[];
};
export type ChangeDraft = (change: Partial<McpDraft>) => void;

export function createDraft(transport: "http" | "local", original?: McpDefinition): McpDraft {
  const draft: McpDraft = {
    id: draftId(),
    name: "",
    initialName: "",
    scope: { kind: "user" },
    expectedRevision: null,
    transport,
    url: "",
    program: "",
    argumentsText: "[]",
    directory: "",
    protocol: "2026-07-28",
    headers: [],
  };
  if (original === undefined) {
    return draft;
  }
  draft.id = original.id;
  draft.name = original.name;
  draft.initialName = original.name;
  draft.scope = original.scope;
  draft.expectedRevision = original.revision;
  draft.protocol = original.protocol;
  if (original.endpoint.kind === "http") {
    draft.url = original.endpoint.url;
    draft.headers = Object.entries(original.endpoint.headers).map(([name, value]) => ({
      id: draftId(),
      name,
      value,
    }));
  } else {
    draft.program = original.endpoint.program;
    draft.argumentsText = JSON.stringify(original.endpoint.arguments, null, 2);
    draft.directory = original.endpoint.directory ?? "";
  }
  return draft;
}

export function draftDefinition(draft: McpDraft): McpDefinition {
  const names = new Set<string>();
  for (const header of draft.headers) {
    const key = header.name.trim().toLowerCase();
    if (key === "" || names.has(key)) {
      throw new Error("Enter a unique name for each header, or remove the empty row.");
    }
    names.add(key);
  }
  const args: unknown = JSON.parse(draft.argumentsText);
  if (!Array.isArray(args) || !args.every((value: unknown) => typeof value === "string")) {
    throw new TypeError("Enter arguments as a JSON array of strings.");
  }
  return {
    id: draft.id,
    name: draft.name.trim(),
    scope: draft.scope,
    enabled: false,
    revision: draft.expectedRevision ?? 1,
    protocol: draft.protocol,
    endpoint:
      draft.transport === "http"
        ? {
            kind: "http",
            url: draft.url.trim(),
            headers: Object.fromEntries(
              draft.headers.map((header) => [header.name.trim(), header.value]),
            ),
          }
        : {
            kind: "local",
            program: draft.program.trim(),
            arguments: args,
            directory: draft.directory.trim() || null,
          },
  };
}
