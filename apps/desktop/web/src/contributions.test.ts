import { expect, it, vi } from "vitest";
import { ContributionRegistry, type ClientExtension, type ContributionContext, type ScopedClientApi } from "./contributions";
const context: ContributionContext = { client: "browser", nodeId: "node", conversationId: "chat", projectId: "project", worktreeId: "tree", connected: true, composerReady: true };
const extension = (id = "test"): ClientExtension => ({ id, manifest: { version: 1, commands: [{ id: "run", title: "Run", when: [] }], slots: [{ id: "button", title: "Run", slot: "composer.actions", kind: "action", command: "run", order: 0, when: [], enabledWhen: ["composerReady"], clients: [] }], screens: [{ id: "screen", title: "Screen", clients: [] }], panels: [{ id: "panel", title: "Panel", clients: [] }] }, handlers: { run: vi.fn() } });
function setup() { const registry = new ContributionRegistry(); const insertText = vi.fn(), openSurface = vi.fn(), error = vi.fn(); registry.configure({ insertText, openSurface, error }); registry.setContext(context); return { registry, insertText, openSurface, error }; }
it("reserves host-only identities while allowing runtime extensions with the same local name", async () => {
  const { registry } = setup();
  const builtin = extension("@swarmie.dictation"), runtime = extension("dictation");
  expect(() => registry.register(builtin)).toThrow("identity");
  expect(() => registry.registerBuiltin(runtime)).toThrow("identity");
  const remove = registry.registerBuiltin(builtin);
  registry.register(runtime);
  await registry.execute("@swarmie.dictation/run");
  await registry.execute("dictation/run");
  expect(builtin.handlers!.run).toHaveBeenCalledOnce();
  expect(runtime.handlers!.run).toHaveBeenCalledOnce();
  remove();
  expect(registry.canExecute("@swarmie.dictation/run")).toBe(false);
  expect(registry.canExecute("dictation/run")).toBe(true);
});
it("validates atomically, sorts by order and qualified identity, and rejects collisions", () => {
  const { registry } = setup(); const a = extension("a"), b = extension("b");
  registry.register(b); registry.register(a);
  expect(registry.entries("composer.actions").map((entry) => entry.key)).toEqual(["a/button", "b/button"]);
  expect(() => registry.register(a)).toThrow("already registered");
  expect(() => registry.register({ ...extension("bad"), manifest: { ...a.manifest, version: 2 } })).toThrow("version");
  expect(() => registry.register({ ...extension("bad"), manifest: { ...a.manifest, slots: [{ ...a.manifest.slots![0], slot: "composer.status" }] } })).toThrow("type");
  expect(registry.entries("composer.actions")).toHaveLength(2);
});
it("honors clients, visibility, enabled predicates and dispatches the same command with supplied message context", async () => {
  const { registry } = setup(); const test = extension(); test.manifest.slots![0].when = ["message"]; test.manifest.slots![0].clients = ["browser"];
  registry.register(test);
  expect(registry.entries("composer.actions")).toEqual([]);
  const message = { ...context, messageId: "message" };
  expect(registry.entries("composer.actions", message)[0].enabled).toBe(true);
  expect(registry.entries("composer.actions", { ...message, client: "desktop" })).toEqual([]);
  expect(registry.entries("composer.actions", { ...message, composerReady: false })[0].enabled).toBe(false);
  await registry.execute("test/run", message);
  expect(test.handlers!.run).toHaveBeenCalledOnce();
  expect(vi.mocked(test.handlers!.run).mock.calls[0][0].context.messageId).toBe("message");
});
it("guards captured APIs after switching away and back, node changes, deactivation and composer unavailability", async () => {
  const { registry, insertText } = setup(); registry.register(extension());
  const api = registry.api("test"); api.insertText("first");
  registry.setContext({ ...context, conversationId: "other" }); registry.setContext(context);
  expect(api.signal.aborted).toBe(true); expect(() => api.insertText("late")).toThrow("expired");
  const nodeApi = registry.api("test"); registry.setContext({ ...context, nodeId: "other" }); expect(nodeApi.signal.aborted).toBe(true);
  registry.setContext(context); const closed = registry.api("test"); registry.setContext({ ...context, composerReady: false }); expect(() => closed.insertText("late")).toThrow("unavailable");
  expect(insertText).toHaveBeenCalledExactlyOnceWith(context, "first");
});
it("owns navigation, disposes event listeners and command scopes, and allows clean reactivation", async () => {
  const { registry, openSurface } = setup(); const cleanup = vi.fn(), event = vi.fn(); const test = extension();
  test.activate = ({ on }) => { on("surface.opened", event); return cleanup; };
  const off = registry.register(test); const api = registry.api("test");
  api.openScreen("screen"); api.openPanel("panel"); expect(openSurface.mock.calls.map((call) => call.slice(0, 2))).toEqual([["screen", "test/screen"], ["panel", "test/panel"]]);
  expect(() => api.openPanel("someone-else/panel")).toThrow("Unknown");
  registry.emit({ type: "surface.opened", kind: "panel", id: "test/panel", context }); expect(event).toHaveBeenCalledOnce();
  off(); expect(api.signal.aborted).toBe(true); expect(cleanup).toHaveBeenCalledOnce();
  expect(registry.surfaces("panel")).toEqual([]); await expect(registry.execute("test/run")).rejects.toThrow("unavailable");
  registry.emit({ type: "surface.opened", kind: "panel", id: "test/panel", context }); expect(event).toHaveBeenCalledOnce();
  const second = registry.register(extension()); off(); expect(registry.entries("composer.actions")).toHaveLength(1); second();
});
it("aborts an asynchronous command and prevents late text after unregistering", async () => {
  const { registry, insertText } = setup(); let api!: ScopedClientApi; let finish!: () => void;
  const test = extension(); test.handlers = { run: (scope) => { api = scope; return new Promise<void>((resolve) => { finish = resolve; }); } };
  const off = registry.register(test); const pending = registry.execute("test/run"); off();
  expect(api.signal.aborted).toBe(true); expect(() => api.insertText("late")).toThrow("expired"); finish(); await pending; expect(insertText).not.toHaveBeenCalled();
});
it("does not let dynamic state re-enable a disabled context", () => {
  const { registry } = setup(); registry.register(extension()); registry.update("test", "button", { enabled: true, title: "Updated" });
  expect(registry.entries("composer.actions", { ...context, composerReady: false })[0]).toMatchObject({ enabled: false, title: "Updated" });
});
it("rejects shortcut collisions and keeps command enabled state consistent across triggers", async () => {
  const { registry } = setup(); const first = extension("first"); first.manifest.commands![0].shortcut = "Mod+Shift+i";
  registry.register(first); const other = extension("other"); other.manifest.commands![0].shortcut = "mod+shift+i";
  expect(() => registry.register(other)).toThrow("Shortcut already registered");
  registry.setCommandEnabled("first/run", false); expect(registry.commands()).toEqual([]);
  expect(registry.entries("composer.actions")[0].enabled).toBe(false); await expect(registry.execute("first/run")).rejects.toThrow("unavailable");
  registry.setCommandEnabled("first/run", true); await registry.execute("first/run"); expect(first.handlers!.run).toHaveBeenCalledOnce();
});
