import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode, useState } from "react";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { CommandPalette } from "../src/command-palette";
import { ContributionHost, ContributionSlot, ContributionSurface } from "../src/contribution-host";
import { Conversation } from "../src/conversation";
import { newThread } from "../src/protocol";
import { WorkspaceStore } from "../src/workspace";
import type { Workspace } from "../src/workspace";
import { contributionDemo } from "./contributions";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function setup() {
  const thread = { ...newThread("project", "worktree", "gemini"), draft: "hello world" };
  const store = new WorkspaceStore();
  const state: Workspace = {
    ...store.getSnapshot(),
    connected: true,
    extensions: [],
    threads: [thread],
    activeId: thread.id,
    projects: [],
    harnesses: [],
  };
  vi.spyOn(store, "getSnapshot").mockReturnValue(state);
  vi.spyOn(store, "active").mockReturnValue(thread);
  const error = vi.spyOn(store, "setError").mockImplementation(() => undefined);
  const update = vi.spyOn(store, "updateThread").mockImplementation(() => undefined);
  return { thread, store, error, update };
}
it("renders slots, filters commands into the palette, and routes buttons and shortcuts to the same handler", async () => {
  const { thread, store } = setup();
  const handler = vi.fn<() => void>();
  const demo = {
    ...contributionDemo,
    extension: {
      ...contributionDemo.extension,
      handlers: { ...contributionDemo.extension.handlers, insert: handler },
    },
  };
  const contributions = [demo];
  function Fixture() {
    const [paletteOpen, setPaletteOpen] = useState(true);
    return (
      <StrictMode>
        <ContributionHost
          store={store}
          thread={thread}
          ready
          extensions={[]}
          openSurface={vi.fn<() => void>()}
          clientContributions={contributions}
        >
          <ContributionSlot name="composer.actions" />
          <ContributionSlot name="composer.status" />
          {paletteOpen && (
            <CommandPalette
              store={store}
              onClose={() => {
                setPaletteOpen(false);
              }}
              onPanel={vi.fn<() => void>()}
              onOpenProject={vi.fn<() => void>()}
            />
          )}
        </ContributionHost>
      </StrictMode>
    );
  }
  render(<Fixture />);
  expect(screen.getByText("Demo • Ready to insert")).toBeTruthy();
  // The palette is modal, so dispatch from its entry first.
  await userEvent.click(screen.getByText("Insert example at cursor"));
  expect(handler).toHaveBeenCalledOnce();
  // Close modal through Escape, then exercise the same handler's keyboard trigger.
  await userEvent.keyboard("{Escape}");
  await userEvent.keyboard("{Control>}{Shift>}i{/Shift}{/Control}");
  expect(handler).toHaveBeenCalledTimes(2);
});
it("inserts a contribution at the real composer selection and records the updated draft", async () => {
  const { thread, store, update, error } = setup();
  render(
    <ContributionHost
      store={store}
      thread={thread}
      ready
      extensions={[]}
      openSurface={vi.fn<() => void>()}
      clientContributions={[contributionDemo]}
    >
      <Conversation thread={thread} store={store} ready controls={null} />
    </ContributionHost>,
  );
  const input = screen.getByRole("textbox", { name: "Message" });
  if (!(input instanceof HTMLTextAreaElement)) {
    throw new Error("Expected the composer textarea");
  }
  input.focus();
  input.setSelectionRange(6, 11);
  input.dispatchEvent(new Event("select", { bubbles: true }));
  await userEvent.click(screen.getByRole("button", { name: "Insert example" }));
  await waitFor(() => {
    expect(input.value).toBe("hello Example text from the contribution demo. ");
  });
  expect(update).toHaveBeenCalledWith(thread.id, {
    draft: "hello Example text from the contribution demo. ",
  });
  expect(error).not.toHaveBeenCalled();
});
it("mounts a scoped panel and emits closed lifecycle when removed", async () => {
  const { thread, store } = setup();
  const event = vi.fn<() => void>();
  const demo = {
    ...contributionDemo,
    extension: {
      ...contributionDemo.extension,
      activate: ({
        on,
      }: Parameters<NonNullable<typeof contributionDemo.extension.activate>>[0]) => {
        on("surface.closed", event);
      },
    },
  };
  const contributions = [demo];
  const view = render(
    <ContributionHost
      store={store}
      thread={thread}
      ready
      extensions={[]}
      openSurface={vi.fn<() => void>()}
      clientContributions={contributions}
    >
      <ContributionSurface kind="panel" id="demo/inspector" onClose={vi.fn<() => void>()} />
    </ContributionHost>,
  );
  await waitFor(() => {
    expect(screen.getByRole("button", { name: "Insert example at cursor" })).toBeTruthy();
  });
  expect(screen.getByText("Refresh demo")).toBeTruthy();
  expect(screen.getByText("Demo panel • Active")).toBeTruthy();
  view.rerender(
    <ContributionHost
      store={store}
      thread={thread}
      ready
      extensions={[]}
      openSurface={vi.fn<() => void>()}
      clientContributions={contributions}
    >
      {null}
    </ContributionHost>,
  );
  expect(event).toHaveBeenCalledOnce();
});
