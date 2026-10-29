import { expect, test } from "bun:test";

import { createDraft, draftDefinition, draftId } from "../src/settings/mcp-draft";

test("MCP drafts and header identities work without the secure-context UUID API", () => {
  const original = Object.getOwnPropertyDescriptor(crypto, "randomUUID");
  Object.defineProperty(crypto, "randomUUID", { configurable: true, value: undefined });
  try {
    const draft = createDraft("http");
    draft.name = "Documents";
    draft.url = "https://example.com/mcp";
    draft.headers = [{ id: draftId(), name: "X-Workspace", value: "documents" }];
    const definition = draftDefinition(draft);
    const reopened = createDraft("http", definition);
    expect(draft.id).toMatch(/^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/u);
    expect(reopened.id).toBe(draft.id);
    expect(reopened.headers[0]?.id).not.toBe(draft.headers[0]?.id);
    expect(reopened.headers[0]?.name).toBe("X-Workspace");
    expect(draftDefinition(reopened)).toEqual(definition);
  } finally {
    if (original === undefined) {
      Reflect.deleteProperty(crypto, "randomUUID");
    } else {
      Object.defineProperty(crypto, "randomUUID", original);
    }
  }
});
