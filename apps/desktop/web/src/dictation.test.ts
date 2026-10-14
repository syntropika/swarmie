// @vitest-environment happy-dom
import { expect, it, vi } from "vitest";
import { createDictationExtension, type TranscriptionProvider } from "./dictation";
import { ContributionRegistry } from "./contributions";
it("shares toggle dispatch, processes final text, and discards callbacks after changing conversations", async () => {
  const registry = new ContributionRegistry(); const insert = vi.fn(), error = vi.fn(), abort = vi.fn(); let callbacks!: Parameters<TranscriptionProvider["start"]>[0];
  const stop = vi.fn(); const provider: TranscriptionProvider = { id: "fixture", available: () => true, start: (options) => { callbacks = options; return { stop, abort }; } };
  registry.configure({ insertText: insert, error, openSurface: vi.fn() });
  registry.setContext({ client: "browser", nodeId: "node", conversationId: "one", connected: true, composerReady: true });
  const off = registry.registerBuiltin(createDictationExtension(registry, error, provider));
  const pending = registry.execute("@swarmie.dictation/toggle"); expect(registry.entries("composer.actions")[0].title).toBe("Stop dictation");
  await registry.execute("@swarmie.dictation/toggle"); expect(stop).toHaveBeenCalledOnce(); expect(registry.entries("composer.status")[0].text).toBe("Processing dictation…");
  callbacks.text("final transcript"); callbacks.ended(); await pending;
  expect(insert).toHaveBeenCalledOnce(); expect(registry.entries("composer.actions")[0].title).toBe("Dictate message");
  const recording = registry.execute("@swarmie.dictation/toggle"); registry.setContext({ client: "browser", nodeId: "node", conversationId: "two", connected: true, composerReady: true });
  callbacks.text("late transcript"); await recording; expect(abort).toHaveBeenCalledOnce(); expect(insert).toHaveBeenCalledOnce(); off();
});
it("disables unsupported capture and cleans up denied or failed capture", async () => {
  const registry = new ContributionRegistry(), error = vi.fn(); registry.setContext({ client: "browser", conversationId: "one", connected: true, composerReady: true });
  const off = registry.registerBuiltin(createDictationExtension(registry, error, { id: "missing", available: () => false, start: vi.fn() }));
  expect(registry.entries("composer.actions")[0].enabled).toBe(false); off();
  registry.registerBuiltin(createDictationExtension(registry, error, { id: "failed", available: () => true, start: () => { throw new Error("denied"); } }));
  await registry.execute("@swarmie.dictation/toggle"); expect(error).toHaveBeenCalledOnce(); expect(registry.entries("composer.actions")[0].title).toBe("Dictate message");
});
