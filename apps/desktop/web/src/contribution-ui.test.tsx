// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContributionHost, ContributionSlot, ContributionSurface } from "./ContributionHost";
import { ContributionRegistry } from "./contributions";
import { CommandPalette } from "./CommandPalette";
import { Conversation } from "./Conversation";
import { contributionDemo } from "./contributionDemo";
import { newThread } from "./protocol";
import type { WorkspaceStore, Workspace } from "./workspace";
import { StrictMode } from "react";
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
function setup() {
  const thread = { ...newThread("project", "worktree", "gemini"), draft: "hello world" };
  const state = { connected: true, extensions: [], threads: [thread], activeId: thread.id, projects: [], harnesses: [], notifications: {} } as unknown as Workspace;
  const error = vi.fn(), update = vi.fn();
  const store = { getSnapshot: () => state, subscribe: () => () => {}, updateThread: update, setError: error, active: () => thread } as unknown as WorkspaceStore;
  return { thread, store, error, update };
}
it("renders slots, filters commands into the palette, and routes buttons and shortcuts to the same handler", async () => {
  const { thread, store } = setup(); const handler = vi.fn();
  const demo = { ...contributionDemo, extension: { ...contributionDemo.extension, handlers: { ...contributionDemo.extension.handlers, insert: handler } } };
  render(<StrictMode><ContributionHost store={store} thread={thread} ready extensions={[]} openSurface={vi.fn()} demo={demo}>
    <ContributionSlot name="composer.actions" /><ContributionSlot name="composer.status" />
    <CommandPalette store={store} onClose={vi.fn()} onPanel={vi.fn()} onOpenProject={vi.fn()} />
  </ContributionHost></StrictMode>);
  expect(screen.getByText("Demo • Ready to insert")).toBeTruthy();
  // The palette is modal, so dispatch from its entry first.
  await userEvent.click(screen.getByText("Insert example at cursor")); expect(handler).toHaveBeenCalledOnce();
  // Close modal through Escape, then exercise the same handler's keyboard trigger.
  await userEvent.keyboard("{Escape}");
  await userEvent.keyboard("{Control>}{Shift>}i{/Shift}{/Control}");
  expect(handler).toHaveBeenCalledTimes(2);
});
it("inserts a contribution at the real composer selection and records the updated draft", async () => {
  const { thread, store, update, error } = setup();
  render(<ContributionHost store={store} thread={thread} ready extensions={[]} openSurface={vi.fn()} demo={contributionDemo}>
    <Conversation thread={thread} store={store} ready controls={null} />
  </ContributionHost>);
  const input = screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
  input.focus(); input.setSelectionRange(6, 11); input.dispatchEvent(new Event("select", { bubbles: true }));
  await userEvent.click(screen.getByRole("button", { name: "Insert example" }));
  await waitFor(() => expect(input.value).toBe("hello Example text from the contribution demo. "));
  expect(update).toHaveBeenCalledWith(thread.id, { draft: "hello Example text from the contribution demo. " }); expect(error).not.toHaveBeenCalled();
});
it("mounts a scoped panel and emits closed lifecycle when removed", async () => {
  const { thread, store } = setup(); const event = vi.fn();
  const demo = { ...contributionDemo, extension: { ...contributionDemo.extension, activate: ({ on }: Parameters<NonNullable<typeof contributionDemo.extension.activate>>[0]) => { on("surface.closed", event); } } };
  const view = render(<ContributionHost store={store} thread={thread} ready extensions={[]} openSurface={vi.fn()} demo={demo}>
    <ContributionSurface kind="panel" id="demo/inspector" onClose={vi.fn()} />
  </ContributionHost>);
  await waitFor(() => expect(screen.getByRole("button", { name: "Insert example at cursor" })).toBeTruthy());
  expect(screen.getByText("Refresh demo")).toBeTruthy(); expect(screen.getByText("Demo panel • Active")).toBeTruthy();
  view.rerender(<ContributionHost store={store} thread={thread} ready extensions={[]} openSurface={vi.fn()} demo={demo}>{null}</ContributionHost>);
  expect(event).toHaveBeenCalledOnce();
});
