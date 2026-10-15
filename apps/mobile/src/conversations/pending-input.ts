import type { InputPromptContent } from "@swarmie/contracts";
import { z } from "zod";

const imageSchema = z.object({
  type: z.literal("image"),
  data: z.object({ data: z.string(), mime_type: z.string() }),
});
const pendingSchema = z.object({
  key: z.string(),
  text: z.string(),
  images: z.array(imageSchema),
  state: z.literal("unresolved"),
});
export type PendingInput = z.infer<typeof pendingSchema>;
const storageKey = (scope: string, sessionId: string) => `swarmie:outbox:${scope}:${sessionId}`;

export function inputContent(input: PendingInput): InputPromptContent[] {
  return [{ type: "text", data: input.text }, ...input.images];
}

export type PendingStorage = {
  getItem: (key: string) => Promise<string | null>;
  setItem: (key: string, value: string) => Promise<void>;
  removeItem: (key: string) => Promise<void>;
};
export function createOutbox(storage: PendingStorage, uuid: () => string) {
  const preparing = new Set<string>();
  async function loadPending(scope: string, sessionId: string): Promise<PendingInput | null> {
    const encoded = await storage.getItem(storageKey(scope, sessionId));
    if (encoded === null) {
      return null;
    }
    const value: unknown = JSON.parse(encoded);
    return pendingSchema.parse(value);
  }

  async function prepareInput(
    scope: string,
    sessionId: string,
    text: string,
    images: z.infer<typeof imageSchema>[],
  ): Promise<PendingInput> {
    const identity = storageKey(scope, sessionId);
    if (preparing.has(identity)) {
      throw new Error("Resolve the previous delivery first");
    }
    preparing.add(identity);
    try {
      if (await loadPending(scope, sessionId)) {
        throw new Error("Resolve the previous delivery first");
      }
      const pending: PendingInput = { key: uuid(), text, images, state: "unresolved" };
      await storage.setItem(storageKey(scope, sessionId), JSON.stringify(pending));
      return pending;
    } finally {
      preparing.delete(identity);
    }
  }

  async function clearPending(scope: string, sessionId: string): Promise<void> {
    await storage.removeItem(storageKey(scope, sessionId));
  }

  return { loadPending, prepareInput, clearPending };
}
