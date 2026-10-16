import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import { Conversation } from "~/src/conversation";
import { renderWithQuery as render } from "~/src/test-fixtures";
import { WorkspaceStore } from "~/src/workspace";
import { WorkspaceSearch } from "~/src/workspace-search";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
async function workspace() {
  localStorage.clear();
  const store = new WorkspaceStore();
  await store.ready();
  store.createThread();
  return store;
}
it("filters the shared palette, navigates with wrapping and runs the chosen action once", async () => {
  const store = await workspace();
  const close = vi.fn<(...args: unknown[]) => void>();
  const project = vi.fn<(...args: unknown[]) => void>();
  const panel = vi.fn<(...args: unknown[]) => void>();
  render(<WorkspaceSearch store={store} onClose={close} onOpenProject={project} onPanel={panel} />);
  const input = screen.getByRole("combobox", {
    name: "Search workspace",
  });
  expect(
    screen.getByRole("listbox", {
      name: "Search workspace",
    }),
  ).toBeTruthy();
  await userEvent.type(input, "project");
  expect(screen.getAllByRole("option")).toHaveLength(1);
  await userEvent.keyboard("{Enter}");
  expect(project).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});
it("keeps an empty search open and supports keyboard wrap through commands", async () => {
  const store = await workspace();
  const close = vi.fn<(...args: unknown[]) => void>();
  const panel = vi.fn<(...args: unknown[]) => void>();
  render(
    <WorkspaceSearch
      store={store}
      onClose={close}
      onOpenProject={vi.fn<(...args: unknown[]) => void>()}
      onPanel={panel}
    />,
  );
  const input = screen.getByRole("combobox", {
    name: "Search workspace",
  });
  await userEvent.type(input, "no-such-command");
  await userEvent.keyboard("{Enter}");
  expect(close).not.toHaveBeenCalled();
  expect(screen.getByText("No results for “no-such-command”.")).toBeTruthy();
  await userEvent.clear(input);
  await userEvent.keyboard("{ArrowUp}{Enter}");
  expect(panel).toHaveBeenCalledExactlyOnceWith("settings");
});
it("copies the correct response through Elements actions without quote or unsupported actions", async () => {
  const store = await workspace();
  const write = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: write,
    },
  });
  const thread = {
    ...requireFixtureValue(store.active()),
    messages: [
      {
        id: "first",
        role: "assistant" as const,
        content: [
          {
            type: "text" as const,
            text: "First response",
          },
        ],
      },
      {
        id: "second",
        role: "assistant" as const,
        content: [
          {
            type: "text" as const,
            text: "Second response",
          },
        ],
      },
    ],
  };
  render(<Conversation thread={thread} store={store} controls={null} ready />);
  expect(
    screen.queryByRole("button", {
      name: /Quote/u,
    }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Mark response helpful",
    }),
  ).toBeNull();
  expect(
    screen.queryByRole("button", {
      name: "Regenerate response",
    }),
  ).toBeNull();
  await userEvent.click(
    screen.getAllByRole("button", {
      name: "Copy",
    })[1],
  );
  await waitFor(() => {
    expect(write).toHaveBeenCalledExactlyOnceWith("Second response");
  });
  expect(
    screen.getByRole("button", {
      name: "Copied",
    }),
  ).toBeTruthy();
  expect(
    screen.getAllByRole("button", {
      name: "Copy",
    }),
  ).toHaveLength(1);
});
it("places the saved user time and copy outside the bubble without an edit action", async () => {
  const store = await workspace();
  const date = new Date("2026-10-04T19:02:00Z");
  const write = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: write,
    },
  });
  const thread = {
    ...requireFixtureValue(store.active()),
    capabilities: {
      rollback: {
        availability: "available" as const,
        source: "harness" as const,
        evidence: "declared" as const,
      },
    },
    messages: [
      {
        id: "user-original",
        role: "user" as const,
        createdAt: date,
        content: [
          {
            type: "text" as const,
            text: "Original message",
          },
        ],
      },
    ],
  };
  const view = render(<Conversation thread={thread} store={store} controls={null} ready />);
  const bubble = requireFixtureValue(
    screen.getByText("Original message").closest('[data-slot="user-message-bubble"]'),
  );
  const copy = screen.getByRole("button", {
    name: "Copy",
  });
  expect(bubble.contains(copy)).toBe(false);
  expect(
    screen.queryByRole("button", {
      name: "Edit message",
    }),
  ).toBeNull();
  expect(view.container.querySelector("time")?.getAttribute("datetime")).toBe(date.toISOString());
  await userEvent.click(copy);
  await waitFor(() => {
    expect(write).toHaveBeenCalledExactlyOnceWith("Original message");
  });
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
