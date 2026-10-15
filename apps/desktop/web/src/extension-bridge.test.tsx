import { cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect } from "react";
// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

import {
  ContributionHost,
  ContributionSurface,
  ContributionSlot,
  useContributions,
} from "./contribution-host";
import { CustomExtensionView } from "./custom-extension-view";
import { extensionDocument } from "./extension-frame";
import { newThread } from "./protocol";
import { renderWithQuery as render, fixtureWorkspace } from "./test-fixtures";
import { WorkspaceStore } from "./workspace";

function matchingObject(value: Record<string, unknown>): unknown {
  return expect.objectContaining(value);
}
function matchingString(value: string): unknown {
  return expect.stringContaining(value);
}
function frameChannel(frame: HTMLIFrameElement): string {
  const match = /const channel = (?<channel>"[^" ]+")/u.exec(frame.srcdoc);
  const value: unknown = JSON.parse(requireFixtureValue(match?.groups?.channel));
  if (typeof value !== "string") {
    throw new TypeError("Expected the renderer channel to be a string.");
  }
  return value;
}
afterEach(cleanup);
it("keeps runtime dictation commands and surfaces separate from built-in dictation", async () => {
  const thread = newThread("project", "tree", "codex");
  const extension = {
    id: "dictation",
    commands: [
      {
        name: "render",
        title: "Render",
        mutates: false,
      },
      {
        name: "echo",
        title: "Echo",
        mutates: false,
      },
    ],
    ui: {
      version: 1,
      commands: [
        {
          id: "echo",
          title: "Runtime dictation",
          runtimeCommand: "echo",
          when: [],
        },
      ],
      slots: [
        {
          id: "echo",
          title: "Runtime dictation",
          kind: "action" as const,
          slot: "composer.actions" as const,
          command: "echo",
          clients: [],
          when: [],
          enabledWhen: [],
          order: 0,
        },
      ],
      screens: [
        {
          id: "settings",
          title: "Runtime dictation settings",
          command: "render",
          clients: [],
        },
      ],
      panels: [],
    },
  };
  const state = fixtureWorkspace({
    connected: true,
    threads: [thread],
    extensions: [extension],
  });
  const invoke = vi.fn().mockResolvedValue("<p>Runtime settings</p>");
  const error = vi.fn<(...args: unknown[]) => void>();
  const store = Object.assign(new WorkspaceStore(), {
    getSnapshot: () => state,
    invokeExtension: invoke,
    setError: error,
  });
  render(
    <ContributionHost
      store={store}
      thread={thread}
      ready
      extensions={[extension]}
      openSurface={vi.fn<(...args: unknown[]) => void>()}
    >
      <ContributionSlot name="composer.actions" />
      <ContributionSurface
        kind="screen"
        id="dictation/settings"
        onClose={vi.fn<(...args: unknown[]) => void>()}
      />
    </ContributionHost>,
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Runtime dictation",
    }),
  );
  expect(
    screen.getByRole("button", {
      name: "Dictate message",
    }),
  ).toBeTruthy();
  expect(invoke).toHaveBeenCalledWith(
    "dictation",
    "echo",
    matchingObject({
      conversationId: thread.id,
    }),
  );
  expect(await screen.findByTitle("Runtime dictation settings")).toBeTruthy();
  expect(invoke).toHaveBeenCalledWith(
    "dictation",
    "render",
    matchingObject({ projectId: "tree" }),
    expect.any(Function),
  );
  expect(error).not.toHaveBeenCalled();
});
it("keeps legacy command views sandboxed and rejects unadvertised or foreign-frame calls", async () => {
  const invoke = vi.fn().mockResolvedValue("<h1>Legacy tool</h1>");
  const thread = {
    ...newThread("project", "tree", "codex"),
    sessionId: "session",
  };
  const extension = {
    id: "legacy",
    commands: [
      {
        name: "render",
        title: "Render",
        mutates: false,
      },
      {
        name: "inspect",
        title: "Inspect",
        mutates: false,
      },
    ],
  };
  render(
    <CustomExtensionView
      store={Object.assign(new WorkspaceStore(), {
        invokeExtension: invoke,
      })}
      thread={thread}
      extension={extension}
      view={{
        id: "legacy",
        title: "Legacy",
        kind: "web",
        command: "render",
      }}
    />,
  );
  const frame = await screen.findByTitle<HTMLIFrameElement>("Legacy");
  expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
  expect(frame.srcdoc).toContain("connect-src 'none'");
  const channel = frameChannel(frame);
  const reply = vi.spyOn(requireFixtureValue(frame.contentWindow), "postMessage");
  window.dispatchEvent(
    new MessageEvent("message", {
      source: window,
      data: {
        channel,
        id: 1,
        command: "inspect",
        arguments: {},
      },
    }),
  );
  expect(invoke).toHaveBeenCalledOnce();
  window.dispatchEvent(
    new MessageEvent("message", {
      source: frame.contentWindow,
      data: {
        channel,
        id: 2,
        command: "unknown",
        arguments: {},
      },
    }),
  );
  expect(reply).toHaveBeenCalledWith(
    matchingObject({
      error: matchingString("not advertised"),
    }),
    "*",
  );
  window.dispatchEvent(
    new MessageEvent("message", {
      source: frame.contentWindow,
      data: {
        channel,
        id: 3,
        command: "inspect",
        arguments: {
          projectId: "tree",
        },
      },
    }),
  );
  await waitFor(() => {
    expect(invoke).toHaveBeenCalledTimes(2);
  });
  expect(invoke).toHaveBeenLastCalledWith("legacy", "inspect", {
    projectId: "tree",
  });
});
it("adds only versioned scoped bridge operations and bounds renderer HTML", () => {
  const source = extensionDocument("<p>Tool</p>", "channel");
  expect(source).toContain("version: 1");
  expect(source).toContain("client.insertText");
  expect(source).toContain("client.openScreen");
  expect(source).toContain("client.openPanel");
  expect(source).toContain("event.source !== parent");
  expect(source).toContain("pagehide");
  expect(() => extensionDocument("x".repeat(1024 * 1024 + 1), "channel")).toThrow("under 1 MB");
});
it("dispatches runtime contributions and limits the v1 frame composer API to a live ready context", async () => {
  const thread = newThread("project", "tree", "opencode");
  const extension = {
    id: "runtime",
    commands: [
      {
        name: "render",
        title: "Render",
        mutates: false,
      },
      {
        name: "echo",
        title: "Echo",
        mutates: false,
      },
    ],
    ui: {
      version: 1,
      commands: [
        {
          id: "echo",
          title: "Echo",
          runtimeCommand: "echo",
          when: ["connected" as const],
        },
      ],
      slots: [
        {
          id: "echo",
          title: "Echo",
          kind: "action" as const,
          slot: "composer.actions" as const,
          command: "echo",
          clients: [],
          when: [],
          enabledWhen: [],
          order: 0,
        },
        {
          id: "project",
          title: "Echo project",
          kind: "action" as const,
          slot: "project.actions" as const,
          command: "echo",
          clients: [],
          when: [],
          enabledWhen: [],
          order: 0,
        },
      ],
      panels: [
        {
          id: "panel",
          title: "Runtime panel",
          command: "render",
          clients: [],
        },
      ],
      screens: [],
    },
  };
  const state = fixtureWorkspace({
    connected: true,
    threads: [thread],
    extensions: [extension],
  });
  const invoke = vi
    .fn()
    .mockImplementation(async (_id: string, command: string, args: unknown) =>
      command === "render" ? "<p>Runtime fixture</p>" : args,
    );
  const store = Object.assign(new WorkspaceStore(), {
    getSnapshot: () => state,
    invokeExtension: invoke,
    setError: vi.fn<(...args: unknown[]) => void>(),
  });
  const insert = vi.fn<(...args: unknown[]) => void>();
  function Binding() {
    const host = requireFixtureValue(useContributions());
    const { bindComposer } = host;
    useEffect(() => bindComposer(thread.id, insert), [bindComposer]);
    return null;
  }
  const content = (ready: boolean, enabled = true) => (
    <ContributionHost
      store={store}
      thread={thread}
      ready={ready}
      extensions={enabled ? [extension] : []}
      openSurface={vi.fn<(...args: unknown[]) => void>()}
    >
      <Binding />
      <ContributionSlot name="composer.actions" />
      <ContributionSlot
        name="project.actions"
        context={{
          projectId: "row-project",
          worktreeId: undefined,
          conversationId: undefined,
          sessionId: undefined,
          composerReady: false,
        }}
      />
      <ContributionSurface
        kind="panel"
        id="runtime/panel"
        onClose={vi.fn<(...args: unknown[]) => void>()}
      />
    </ContributionHost>
  );
  const view = render(content(true));
  const frame = await screen.findByTitle<HTMLIFrameElement>("Runtime panel");
  await userEvent.click(
    screen.getByRole("button", {
      name: "Echo",
    }),
  );
  expect(invoke).toHaveBeenCalledWith(
    "runtime",
    "echo",
    matchingObject({
      conversationId: thread.id,
      projectId: "tree",
    }),
  );
  await userEvent.click(
    screen.getByRole("button", {
      name: "Echo project",
    }),
  );
  expect(invoke).toHaveBeenCalledWith(
    "runtime",
    "echo",
    matchingObject({
      projectId: "row-project",
      projectRecordId: "row-project",
      worktreeId: undefined,
      clientContext: matchingObject({
        projectId: "row-project",
        conversationId: undefined,
      }),
    }),
  );
  const channel = frameChannel(frame);
  const reply = vi.spyOn(requireFixtureValue(frame.contentWindow), "postMessage");
  const send = (id: number, operation: string, payload = {}) =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: frame.contentWindow,
        data: {
          channel,
          id,
          operation,
          ...payload,
        },
      }),
    );
  send(1, "client.insertText", {
    text: "Scoped transcript",
  });
  expect(insert).toHaveBeenCalledWith("Scoped transcript", undefined);
  send(2, "client.openPanel", {
    surfaceId: "foreign/panel",
  });
  expect(reply).toHaveBeenCalledWith(
    matchingObject({
      error: matchingString("Unknown extension surface"),
    }),
    "*",
  );
  view.rerender(content(false));
  // Reloaded frame scopes must still reject insertion while the composer is unavailable.
  await waitFor(() => {
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "Dictate message",
      }).disabled,
    ).toBe(true);
  });
  const current = await screen.findByTitle<HTMLIFrameElement>("Runtime panel");
  const currentChannel = frameChannel(current);
  const currentReply = vi.spyOn(requireFixtureValue(current.contentWindow), "postMessage");
  window.dispatchEvent(
    new MessageEvent("message", {
      source: current.contentWindow,
      data: {
        channel: currentChannel,
        id: 3,
        operation: "client.insertText",
        text: "Late",
      },
    }),
  );
  expect(currentReply).toHaveBeenCalledWith(
    matchingObject({
      error: matchingString("unavailable"),
    }),
    "*",
  );
  expect(insert).toHaveBeenCalledOnce();
  view.rerender(content(false, false));
  send(4, "client.insertText", {
    text: "Removed",
  });
  expect(insert).toHaveBeenCalledOnce();
});
function requireFixtureValue<T>(value: T | null | undefined): T {
  if (value === undefined || value === null) {
    throw new Error("Expected the fixture value to be present.");
  }
  return value;
}
