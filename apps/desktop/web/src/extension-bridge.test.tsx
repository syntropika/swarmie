// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { ContributionHost, ContributionSurface, ContributionSlot, useContributions } from "./ContributionHost";
import userEvent from "@testing-library/user-event";
import { CustomExtensionView } from "./CustomExtensionView";
import { extensionDocument } from "./extensionFrame";
import { newThread } from "./protocol";
import type { WorkspaceStore, Workspace } from "./workspace";
afterEach(cleanup);
it("keeps legacy command views sandboxed and rejects unadvertised or foreign-frame calls", async () => {
  const invoke = vi.fn().mockResolvedValue("<h1>Legacy tool</h1>");
  const thread = { ...newThread("project", "tree", "codex"), sessionId: "session" };
  const extension = { id: "legacy", commands: [{ name: "render", title: "Render", mutates: false }, { name: "inspect", title: "Inspect", mutates: false }] };
  render(<CustomExtensionView store={{ invokeExtension: invoke } as unknown as WorkspaceStore} thread={thread} extension={extension} view={{ id: "legacy", title: "Legacy", kind: "web", command: "render" }} />);
  const frame = await screen.findByTitle("Legacy") as HTMLIFrameElement;
  expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
  expect(frame.srcdoc).toContain("connect-src 'none'");
  const channel = JSON.parse(frame.srcdoc.match(/const channel = ("[^"]+")/)![1]);
  const reply = vi.spyOn(frame.contentWindow!, "postMessage");
  window.dispatchEvent(new MessageEvent("message", { source: window, data: { channel, id: 1, command: "inspect", arguments: {} } }));
  expect(invoke).toHaveBeenCalledOnce();
  window.dispatchEvent(new MessageEvent("message", { source: frame.contentWindow, data: { channel, id: 2, command: "unknown", arguments: {} } }));
  expect(reply).toHaveBeenCalledWith(expect.objectContaining({ error: expect.stringContaining("not advertised") }), "*");
  window.dispatchEvent(new MessageEvent("message", { source: frame.contentWindow, data: { channel, id: 3, command: "inspect", arguments: { projectId: "tree" } } }));
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  expect(invoke).toHaveBeenLastCalledWith("legacy", "inspect", { projectId: "tree" });
});
it("adds only versioned scoped bridge operations and bounds renderer HTML", () => {
  const source = extensionDocument("<p>Tool</p>", "channel");
  expect(source).toContain("version: 1"); expect(source).toContain("client.insertText"); expect(source).toContain("client.openScreen"); expect(source).toContain("client.openPanel");
  expect(source).toContain("event.source !== parent"); expect(source).toContain("pagehide");
  expect(() => extensionDocument("x".repeat(1024 * 1024 + 1), "channel")).toThrow("under 1 MB");
});

it("dispatches runtime contributions and limits the v1 frame composer API to a live ready context", async () => {
  const thread = newThread("project", "tree", "opencode");
  const extension = { id: "runtime", commands: [
    { name: "render", title: "Render", mutates: false }, { name: "echo", title: "Echo", mutates: false },
  ], ui: { version: 1, commands: [{ id: "echo", title: "Echo", runtimeCommand: "echo", when: ["connected" as const] }],
    slots: [{ id: "echo", title: "Echo", kind: "action" as const, slot: "composer.actions" as const, command: "echo", clients: [], when: [], enabledWhen: [], order: 0 }, { id: "project", title: "Echo project", kind: "action" as const, slot: "project.actions" as const, command: "echo", clients: [], when: [], enabledWhen: [], order: 0 }],
    panels: [{ id: "panel", title: "Runtime panel", command: "render", clients: [] }], screens: [] } };
  const state = { connected: true, threads: [thread], extensions: [extension] } as unknown as Workspace;
  const invoke = vi.fn().mockImplementation(async (_id, command, args) => command === "render" ? "<p>Runtime fixture</p>" : args);
  const store = { getSnapshot: () => state, invokeExtension: invoke, setError: vi.fn() } as unknown as WorkspaceStore;
  const insert = vi.fn();
  function Binding() { const host = useContributions()!; useEffect(() => host.bindComposer(thread.id, insert), [host.registry]); return null; }
  const content = (ready: boolean, enabled = true) => <ContributionHost store={store} thread={thread} ready={ready} extensions={enabled ? [extension] : []} openSurface={vi.fn()}>
    <Binding /><ContributionSlot name="composer.actions" /><ContributionSlot name="project.actions" context={{ projectId: "row-project", worktreeId: undefined, conversationId: undefined, sessionId: undefined, composerReady: false }} /><ContributionSurface kind="panel" id="runtime/panel" onClose={vi.fn()} />
  </ContributionHost>;
  const view = render(content(true));
  const frame = await screen.findByTitle("Runtime panel") as HTMLIFrameElement;
  await userEvent.click(screen.getByRole("button", { name: "Echo" }));
  expect(invoke).toHaveBeenCalledWith("runtime", "echo", expect.objectContaining({ conversationId: thread.id, projectId: "tree" }));
  await userEvent.click(screen.getByRole("button", { name: "Echo project" }));
  expect(invoke).toHaveBeenCalledWith("runtime", "echo", expect.objectContaining({ projectId: "row-project", projectRecordId: "row-project", worktreeId: undefined, clientContext: expect.objectContaining({ projectId: "row-project", conversationId: undefined }) }));
  const channel = JSON.parse(frame.srcdoc.match(/const channel = ("[^"]+")/)![1]);
  const reply = vi.spyOn(frame.contentWindow!, "postMessage");
  const send = (id: number, operation: string, payload = {}) => window.dispatchEvent(new MessageEvent("message", { source: frame.contentWindow, data: { channel, id, operation, ...payload } }));
  send(1, "client.insertText", { text: "Scoped transcript" }); expect(insert).toHaveBeenCalledWith("Scoped transcript", undefined);
  send(2, "client.openPanel", { surfaceId: "foreign/panel" }); expect(reply).toHaveBeenCalledWith(expect.objectContaining({ error: expect.stringContaining("Unknown extension surface") }), "*");
  view.rerender(content(false));
  // Reloaded frame scopes must still reject insertion while the composer is unavailable.
  await waitFor(() => expect((screen.getByRole("button", { name: "Dictate message" }) as HTMLButtonElement).disabled).toBe(true));
  const current = await screen.findByTitle("Runtime panel") as HTMLIFrameElement;
  const currentChannel = JSON.parse(current.srcdoc.match(/const channel = ("[^"]+")/)![1]);
  const currentReply = vi.spyOn(current.contentWindow!, "postMessage");
  window.dispatchEvent(new MessageEvent("message", { source: current.contentWindow, data: { channel: currentChannel, id: 3, operation: "client.insertText", text: "Late" } }));
  expect(currentReply).toHaveBeenCalledWith(expect.objectContaining({ error: expect.stringContaining("unavailable") }), "*"); expect(insert).toHaveBeenCalledOnce();
  view.rerender(content(false, false)); send(4, "client.insertText", { text: "Removed" }); expect(insert).toHaveBeenCalledOnce();
});
