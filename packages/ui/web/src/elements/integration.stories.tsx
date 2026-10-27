import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import { ConversationThread } from "./conversation";
import type { ConversationMessageView } from "./conversation";
import { GenerativePresentation } from "./generative";
import { ApprovalCard } from "./index";
import type { ApprovalState } from "./index";

function HostConversation() {
  const [messages, setMessages] = useState<ConversationMessageView[]>([]);
  const [draft, setDraft] = useState("");
  const [running, setRunning] = useState(false);
  return (
    <div className="grid gap-4">
      <ConversationThread
        messages={messages}
        draft={draft}
        isRunning={running}
        onDraftChange={setDraft}
        onSend={() => {
          setMessages((current) => [
            ...current,
            { id: String(current.length), role: "user", content: draft },
          ]);
          setDraft("");
          setRunning(true);
        }}
        onCancel={() => {
          setRunning(false);
        }}
      />
      <output>{running ? "Host received the send request" : "Ready"}</output>
    </div>
  );
}
function HostApproval() {
  const [state, setState] = useState<ApprovalState>("request");
  return (
    <div className="grid gap-4">
      <ApprovalCard
        state={state}
        title="Review a request"
        subtitle="The host requests a decision"
        command="run checks"
        onAllowOnce={() => {
          setState("running");
        }}
        onAlwaysAllow={() => {
          setState("running");
        }}
        onDeny={() => {
          setState("denied");
        }}
      />
      <output aria-label="Host decision">{state}</output>
    </div>
  );
}
function HostForm() {
  const [received, setReceived] = useState("");
  return (
    <div className="grid gap-4">
      <GenerativePresentation
        tree={{
          $type: "Card",
          asForm: true,
          confirm: { label: "Confirm review", $action: { type: "review" } },
          children: [
            { $type: "Input", name: "title", label: "Review title" },
            {
              $type: "RadioGroup",
              name: "scope",
              label: "Scope",
              options: [
                { value: "once", label: "This request" },
                { value: "always", label: "Future requests" },
              ],
              defaultValue: "once",
            },
          ],
        }}
        onAction={(action) => {
          setReceived(JSON.stringify(action));
        }}
      />
      <output>{received}</output>
    </div>
  );
}
const meta = {
  title: "Assistant Elements/Host integration",
  parameters: {
    layout: "padded",
    docs: {
      description: {
        component:
          "Host-owned fixture state verifies callback delivery and HeroUI integration. No assistant runtime, provider, task or model is started.",
      },
    },
  },
} satisfies Meta;
export default meta;
type Story = StoryObj<typeof meta>;
export const Conversation: Story = {
  render: () => <HostConversation />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(
      canvas.getByRole("textbox", { name: "Message" }),
      "Review the public interface",
    );
    await userEvent.click(canvas.getByRole("button", { name: "Send" }));
    await expect(canvas.getByText("Review the public interface")).toBeVisible();
    await expect(canvas.getByRole("status")).toHaveTextContent("Host received");
    await userEvent.click(canvas.getByRole("button", { name: "Stop" }));
    await expect(canvas.getByRole("status")).toHaveTextContent("Ready");
  },
};
export const Approval: Story = {
  render: () => <HostApproval />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(canvas.getByRole("button", { name: "Allow once" }));
    await expect(canvas.getByRole("status", { name: "Host decision" })).toHaveTextContent(
      "running",
    );
  },
};
export const StructuredForm: Story = {
  render: () => <HostForm />,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.type(canvas.getByRole("textbox", { name: "Review title" }), "Public interface");
    await userEvent.click(canvas.getByRole("button", { name: "Confirm review" }));
    await expect(canvas.getByRole("status")).toHaveTextContent('"type":"review"');
    await expect(canvas.getByRole("status")).toHaveTextContent('"title":"Public interface"');
    await expect(canvas.getByRole("status")).toHaveTextContent('"scope":"once"');
  },
};
