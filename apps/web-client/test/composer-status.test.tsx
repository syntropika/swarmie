import { afterEach, expect, test } from "bun:test";

import { createHost } from "@syntropika/tembra-sdk/node";
import { renderToStaticMarkup } from "react-dom/server";

import { ClientContext } from "../src/client/context";
import { initializeClient } from "../src/client/initialize";
import type { ClientUi } from "../src/client/initialize";
import { MessageRow } from "../src/conversations/message-row";
import { ConversationStatus } from "../src/conversations/status";
import { ToolResults } from "../src/conversations/tool-results";
import { GeneralSettings } from "../src/settings/general";
import { SettingsPage } from "../src/settings/page";
import { SettingsNavigation } from "../src/shell/navigation/settings";

let client: ClientUi | undefined;
afterEach(async () => {
  await client?.close();
  client = undefined;
});

test.each([
  ["unknown", true, true],
  ["unknown", false, false],
  ["failed", true, false],
  ["working", false, false],
] as const)(
  "conversation acknowledgement control: phase=%s, recovery=%s",
  async (phase, canAcknowledge, visible) => {
    const current = await initializeClient(createHost(), { dev: false });
    client = current;
    const chat = current.activeConversation();
    chat.execution.update((value) => ({ ...value, phase, canAcknowledge }));
    await Bun.sleep(5);
    const markup = renderToStaticMarkup(
      <ClientContext value={current}>
        <ConversationStatus chat={chat} />
      </ClientContext>,
    );
    expect(markup.includes("Acknowledge unknown outcome")).toBe(visible);
    expect(markup.includes("does not retry the tool")).toBe(visible);
    expect(markup.includes("A tool may have changed external data.")).toBe(visible);
  },
);

test("working uses the shared activity indicator without claiming a reasoning transcript", async () => {
  const current = await initializeClient(createHost(), { dev: false });
  client = current;
  const chat = current.activeConversation();
  chat.execution.update((value) => ({ ...value, phase: "working", label: "Thinking…" }));
  await Bun.sleep(5);
  const markup = renderToStaticMarkup(
    <ClientContext value={current}>
      <ConversationStatus chat={chat} />
    </ClientContext>,
  );
  expect(markup).toContain('data-slot="thinking-indicator"');
  expect(markup).toContain("Working…");
  expect(markup).not.toContain("Thinking…");
});

test("settled results use the shared disclosure without injecting result markup", () => {
  const markup = renderToStaticMarkup(
    <ToolResults
      messages={[
        { id: "result", role: "tool", text: '<script>alert("fixture")</script>', truncated: true },
      ]}
    />,
  );
  expect(markup).toContain("Tool result");
  expect(markup).toContain('aria-expanded="false"');
  expect(markup).not.toContain("<script>");
  expect(markup).not.toContain("Tool call");
});

test("conversation display preferences have their own settings destination", async () => {
  const current = await initializeClient(createHost(), { dev: false });
  client = current;
  current.navigate({ section: "settings", settings: "conversation" });
  await Bun.sleep(10);
  const page = renderToStaticMarkup(
    <ClientContext value={current}>
      <SettingsPage />
    </ClientContext>,
  );
  expect(page).toContain(">Conversation</h1>");
  expect(page).toContain('aria-label="Tool details"');
  const general = renderToStaticMarkup(
    <ClientContext value={current}>
      <GeneralSettings />
    </ClientContext>,
  );
  expect(general).not.toContain("Tool details");
  const navigation = renderToStaticMarkup(
    <ClientContext value={current}>
      <SettingsNavigation />
    </ClientContext>,
  );
  expect(navigation).toContain("Conversation");
  expect(navigation).toContain('aria-current="page"');
});

test("a shortened message shows the canonical-history notice in the actual message component", async () => {
  const current = await initializeClient(createHost(), { dev: false });
  client = current;
  const chat = current.activeConversation();
  for (const truncated of [true, false]) {
    const markup = renderToStaticMarkup(
      <ClientContext value={current}>
        <MessageRow
          chat={chat}
          message={{ id: "message", role: "assistant", text: "Bounded content", truncated }}
        />
      </ClientContext>,
    );
    expect(markup.includes("This preview is shortened.")).toBe(truncated);
    expect(markup.includes("The full message remains in the conversation history.")).toBe(
      truncated,
    );
    expect(markup).toContain("Bounded content");
  }
});
